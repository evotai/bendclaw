use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bend_base::logx;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use parking_lot::Mutex;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use super::provider::ApiType;
use super::provider::LLMProvider;
use super::provider::ProviderRequest;
use super::provider::ProviderResponse;
use super::response;
use super::stream::collect_response;
use super::stream::ResponseStream;
use super::stream::StreamWriter;
use super::ApiError;
use crate::types::ApiToolParam;
use crate::types::ContentBlock;
use crate::types::ImageContentSource;
use crate::types::Message;
use crate::types::MessageRole;
use crate::types::SystemBlock;
use crate::types::ThinkingConfig;
use crate::types::Usage;

const API_VERSION: &str = "2023-06-01";
const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const PROVIDER_NAME: &str = "anthropic";

#[derive(Debug, Serialize)]
struct AnthropicRequest {
    model: String,
    max_tokens: u64,
    messages: Vec<AnthropicMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<Vec<SystemBlock>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiToolParam>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking: Option<ThinkingConfig>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    custom_headers: HashMap<String, String>,
}

impl AnthropicProvider {
    pub fn new(
        client: Client,
        api_key: String,
        base_url: Option<String>,
        custom_headers: HashMap<String, String>,
    ) -> Self {
        Self {
            client,
            api_key,
            base_url: base_url.unwrap_or_else(|| DEFAULT_BASE_URL.to_string()),
            custom_headers,
        }
    }

    fn messages_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }

    fn build_body(&self, request: ProviderRequest<'_>) -> AnthropicRequest {
        let api_messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                };

                AnthropicMessage {
                    role: role.to_string(),
                    content: serde_json::to_value(&message.content).unwrap_or(Value::Array(vec![])),
                }
            })
            .collect();

        AnthropicRequest {
            model: request.model.to_string(),
            max_tokens: request.max_tokens,
            messages: api_messages,
            system: request.system,
            tools: if request.tools.as_ref().is_none_or(|tools| tools.is_empty()) {
                None
            } else {
                request.tools
            },
            stream: true,
            thinking: request.thinking,
        }
    }

    fn request_builder(&self, body: &AnthropicRequest) -> reqwest::RequestBuilder {
        let mut builder = self
            .client
            .post(self.messages_url())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("content-type", "application/json");

        for (key, value) in &self.custom_headers {
            builder = builder.header(key, value);
        }

        builder.json(body)
    }

    async fn send_request(&self, body: &AnthropicRequest) -> Result<reqwest::Response, ApiError> {
        self.request_builder(body).send().await.map_err(|error| {
            if error.is_timeout() {
                ApiError::Timeout
            } else {
                ApiError::NetworkError(error.to_string())
            }
        })
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    fn api_type(&self) -> ApiType {
        ApiType::AnthropicMessages
    }

    async fn create_message(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<ProviderResponse, ApiError> {
        collect_response(self.create_message_stream(request).await?).await
    }

    async fn create_message_stream(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<ResponseStream, ApiError> {
        let started_at = Instant::now();
        let model = request.model.to_string();
        let message_count = request.messages.len() as u64;
        let tool_count = request.tools.as_ref().map(|tools| tools.len()).unwrap_or(0) as u64;
        let max_tokens = request.max_tokens;
        let body = self.build_body(request);

        logx!(
            info,
            "llm",
            "request",
            provider = PROVIDER_NAME,
            model = %model,
            message_count,
            tool_count,
            max_tokens,
        );

        let response = match self.send_request(&body).await {
            Ok(response) => response,
            Err(error) => {
                logx!(
                    warn,
                    "llm",
                    "request_failed",
                    provider = PROVIDER_NAME,
                    model = %model,
                    error = %error,
                    elapsed_ms = started_at.elapsed().as_millis() as u64,
                );
                return Err(error);
            }
        };

        if !response.status().is_success() {
            let error = response::http_error(response).await;
            logx!(
                warn,
                "llm",
                "response_failed",
                provider = PROVIDER_NAME,
                model = %model,
                error = %error,
                elapsed_ms = started_at.elapsed().as_millis() as u64,
            );
            return Err(error);
        }

        let (writer, stream) = ResponseStream::channel(128);
        writer.set_request_started_at(started_at);
        let model_for_task = model.clone();

        tokio::spawn(async move {
            if let Err(error) =
                stream_anthropic_response(response, writer.clone(), &model_for_task).await
            {
                logx!(
                    warn,
                    "llm",
                    "stream_failed",
                    provider = PROVIDER_NAME,
                    model = %model_for_task,
                    error = %error,
                );
                writer.error(error.to_string()).await;
            }
        });

        Ok(stream)
    }
}

async fn stream_anthropic_response(
    response: reqwest::Response,
    writer: StreamWriter,
    model: &str,
) -> Result<(), ApiError> {
    let raw_body = Arc::new(Mutex::new(Vec::new()));
    let byte_stream = response.bytes_stream().map({
        let writer = writer.clone();
        let raw_body = raw_body.clone();
        move |chunk| match chunk {
            Ok(bytes) => {
                writer.record_chunk(bytes.len());
                raw_body.lock().extend_from_slice(bytes.as_ref());
                Ok(bytes)
            }
            Err(error) => Err(error),
        }
    });

    let mut event_stream = byte_stream.eventsource();
    let mut current_blocks: HashMap<usize, Value> = HashMap::new();
    let mut saw_valid_event = false;
    let mut stop_reason: Option<String> = None;

    while let Some(event) = event_stream.next().await {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                if !saw_valid_event {
                    if let Some(provider_response) =
                        fallback_from_raw_body(&raw_body, parse_anthropic_raw_body)?
                    {
                        writer
                            .emit_response(
                                provider_response,
                                Some(PROVIDER_NAME.to_string()),
                                Some(model.to_string()),
                            )
                            .await;
                        return Ok(());
                    }
                }

                return Err(ApiError::StreamError(format!("SSE parse error: {error}")));
            }
        };

        if event.data.is_empty() {
            continue;
        }
        if event.data == "[DONE]" {
            break;
        }

        let payload: Value = match serde_json::from_str(&event.data) {
            Ok(payload) => payload,
            Err(_) => continue,
        };
        saw_valid_event = true;

        if let Some(error) = response::stream_error(&payload) {
            return Err(error);
        }

        let event_type = payload
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match event_type {
            "message_start" => {
                if let Some(usage) = payload.pointer("/message/usage") {
                    if let Ok(usage) = serde_json::from_value::<Usage>(usage.clone()) {
                        writer.usage(usage).await;
                    }
                }
            }
            "content_block_start" => {
                let index = payload
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                let content_block = payload.get("content_block").cloned();

                if let (Some(index), Some(content_block)) = (index, content_block) {
                    if let Some("tool_use") = content_block.get("type").and_then(Value::as_str) {
                        if let (Some(id), Some(name)) = (
                            content_block.get("id").and_then(Value::as_str),
                            content_block.get("name").and_then(Value::as_str),
                        ) {
                            writer.tool_start(index, id, name).await;
                        }
                    }
                    current_blocks.insert(index, content_block);
                }
            }
            "content_block_delta" => {
                let index = payload
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                let delta = payload.get("delta");

                if let (Some(index), Some(delta)) = (index, delta) {
                    let delta_type = delta
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");

                    match delta_type {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                                writer.text(text).await;
                                append_block_string(&mut current_blocks, index, "text", text);
                            }
                        }
                        "thinking_delta" => {
                            if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                                writer.thinking(thinking).await;
                                append_block_string(
                                    &mut current_blocks,
                                    index,
                                    "thinking",
                                    thinking,
                                );
                            }
                        }
                        "signature_delta" => {
                            if let Some(signature) = delta.get("signature").and_then(Value::as_str)
                            {
                                writer.thinking_signature(signature).await;
                                set_block_string(
                                    &mut current_blocks,
                                    index,
                                    "signature",
                                    signature,
                                );
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial_json) =
                                delta.get("partial_json").and_then(Value::as_str)
                            {
                                writer.tool_delta(index, partial_json).await;
                                append_block_string(
                                    &mut current_blocks,
                                    index,
                                    "_partial_json",
                                    partial_json,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let index = payload
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);

                if let Some(index) = index {
                    emit_pending_tool_end(&writer, index, current_blocks.remove(&index)).await;
                }
            }
            "message_delta" => {
                if let Some(stop) = payload
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                {
                    stop_reason = Some(stop.to_string());
                }

                if let Some(usage) = payload.get("usage") {
                    if let Ok(usage) = serde_json::from_value::<Usage>(usage.clone()) {
                        writer.usage(usage).await;
                    }
                }
            }
            _ => {}
        }
    }

    if !saw_valid_event {
        let provider_response = fallback_from_raw_body(&raw_body, parse_anthropic_raw_body)?
            .ok_or_else(|| {
                ApiError::StreamError("Anthropic upstream returned empty stream".to_string())
            })?;
        writer
            .emit_response(
                provider_response,
                Some(PROVIDER_NAME.to_string()),
                Some(model.to_string()),
            )
            .await;
        return Ok(());
    }

    let mut remaining: Vec<(usize, Value)> = current_blocks.into_iter().collect();
    remaining.sort_by_key(|(index, _)| *index);
    for (index, block) in remaining {
        emit_pending_tool_end(&writer, index, Some(block)).await;
    }

    writer
        .done(
            stop_reason.unwrap_or_else(|| "end_turn".to_string()),
            Some(PROVIDER_NAME.to_string()),
            Some(model.to_string()),
        )
        .await;

    Ok(())
}

fn fallback_from_raw_body<F>(
    raw_body: &Arc<Mutex<Vec<u8>>>,
    parse: F,
) -> Result<Option<ProviderResponse>, ApiError>
where
    F: FnOnce(&str) -> Result<ProviderResponse, ApiError>,
{
    let body = String::from_utf8_lossy(&raw_body.lock()).to_string();
    if body.trim().is_empty() {
        return Ok(None);
    }

    parse(&body).map(Some)
}

fn append_block_string(blocks: &mut HashMap<usize, Value>, index: usize, field: &str, delta: &str) {
    let block = blocks
        .entry(index)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let existing = block.get(field).and_then(Value::as_str).unwrap_or("");
    block[field] = Value::String(format!("{existing}{delta}"));
}

fn set_block_string(blocks: &mut HashMap<usize, Value>, index: usize, field: &str, value: &str) {
    let block = blocks
        .entry(index)
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    block[field] = Value::String(value.to_string());
}

async fn emit_pending_tool_end(writer: &StreamWriter, index: usize, block: Option<Value>) {
    let Some(block) = block else {
        return;
    };

    if block.get("type").and_then(Value::as_str) != Some("tool_use") {
        return;
    }

    let Some(id) = block.get("id").and_then(Value::as_str) else {
        return;
    };
    let Some(name) = block.get("name").and_then(Value::as_str) else {
        return;
    };

    writer
        .tool_end(index, id, name, tool_input_json_string(&block))
        .await;
}

fn tool_input_json_string(block: &Value) -> String {
    if let Some(partial_json) = block.get("_partial_json").and_then(Value::as_str) {
        return partial_json.to_string();
    }

    block
        .get("input")
        .cloned()
        .unwrap_or_else(|| Value::Object(serde_json::Map::new()))
        .to_string()
}

fn parse_anthropic_fallback(body: &str) -> Result<ProviderResponse, ApiError> {
    let value = response::parse_json_body(body, "Anthropic")?;
    if let Some(error) = response::json_stream_error(&value, "Anthropic") {
        return Err(error);
    }
    parse_anthropic_response(&value)
}

fn parse_anthropic_raw_body(body: &str) -> Result<ProviderResponse, ApiError> {
    if body
        .lines()
        .any(|line| line.trim_start().starts_with("data: "))
    {
        return parse_anthropic_sse_body(body);
    }

    parse_anthropic_fallback(body)
}

fn parse_anthropic_sse_body(body: &str) -> Result<ProviderResponse, ApiError> {
    let mut content_blocks = Vec::new();
    let mut usage = Usage::default();
    let mut stop_reason: Option<String> = None;
    let mut current_blocks: HashMap<usize, Value> = HashMap::new();
    let mut saw_valid_sse_event = false;

    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }

        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let payload: Value = match serde_json::from_str(data) {
            Ok(payload) => {
                saw_valid_sse_event = true;
                payload
            }
            Err(_) => continue,
        };

        if let Some(error) = response::stream_error(&payload) {
            return Err(error);
        }

        let event_type = payload
            .get("type")
            .and_then(|value| value.as_str())
            .unwrap_or("");

        match event_type {
            "message_start" => {
                if let Some(usage_value) = payload.pointer("/message/usage") {
                    if let Ok(parsed_usage) = serde_json::from_value::<Usage>(usage_value.clone()) {
                        usage.input_tokens = parsed_usage.input_tokens;
                        usage.cache_creation_input_tokens =
                            parsed_usage.cache_creation_input_tokens;
                        usage.cache_read_input_tokens = parsed_usage.cache_read_input_tokens;
                    }
                }
            }
            "content_block_start" => {
                if let (Some(index), Some(block)) = (
                    payload
                        .get("index")
                        .and_then(|value| value.as_u64())
                        .map(|value| value as usize),
                    payload.get("content_block"),
                ) {
                    current_blocks.insert(index, block.clone());
                }
            }
            "content_block_delta" => {
                let index = payload
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                let delta = payload.get("delta");

                if let (Some(index), Some(delta)) = (index, delta) {
                    let delta_type = delta
                        .get("type")
                        .and_then(|value| value.as_str())
                        .unwrap_or("");

                    match delta_type {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(Value::as_str) {
                                append_block_string(&mut current_blocks, index, "text", text);
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial_json) =
                                delta.get("partial_json").and_then(Value::as_str)
                            {
                                append_block_string(
                                    &mut current_blocks,
                                    index,
                                    "_partial_json",
                                    partial_json,
                                );
                            }
                        }
                        "thinking_delta" => {
                            if let Some(thinking) = delta.get("thinking").and_then(Value::as_str) {
                                append_block_string(
                                    &mut current_blocks,
                                    index,
                                    "thinking",
                                    thinking,
                                );
                            }
                        }
                        "signature_delta" => {
                            if let Some(signature) = delta.get("signature").and_then(Value::as_str)
                            {
                                set_block_string(
                                    &mut current_blocks,
                                    index,
                                    "signature",
                                    signature,
                                );
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let index = payload
                    .get("index")
                    .and_then(|value| value.as_u64())
                    .map(|value| value as usize);
                if let Some(index) = index {
                    if let Some(block) = current_blocks.remove(&index) {
                        if let Some(content_block) = parse_anthropic_content_block(block) {
                            content_blocks.push(content_block);
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(reason) = payload
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
                {
                    stop_reason = Some(reason.to_string());
                }
                if let Some(output_tokens) = payload
                    .get("usage")
                    .and_then(|usage| usage.get("output_tokens"))
                    .and_then(Value::as_u64)
                {
                    usage.output_tokens = output_tokens;
                }
            }
            _ => {}
        }
    }

    if !saw_valid_sse_event {
        return parse_anthropic_fallback(body);
    }

    let mut remaining: Vec<(usize, Value)> = current_blocks.into_iter().collect();
    remaining.sort_by_key(|(index, _)| *index);
    for (_, block) in remaining {
        if let Some(content_block) = parse_anthropic_content_block(block) {
            content_blocks.push(content_block);
        }
    }

    Ok(ProviderResponse {
        message: Message {
            role: MessageRole::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}

fn parse_anthropic_response(value: &Value) -> Result<ProviderResponse, ApiError> {
    let blocks = value
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::StreamError(
                "Anthropic upstream returned JSON body without content".to_string(),
            )
        })?;

    let content = blocks
        .iter()
        .filter_map(|block| parse_anthropic_content_block(block.clone()))
        .collect();

    let usage = value
        .get("usage")
        .cloned()
        .and_then(|usage| serde_json::from_value::<Usage>(usage).ok())
        .unwrap_or_default();

    let stop_reason = value
        .get("stop_reason")
        .and_then(Value::as_str)
        .map(String::from);

    Ok(ProviderResponse {
        message: Message {
            role: MessageRole::Assistant,
            content,
        },
        usage,
        stop_reason,
    })
}

fn parse_anthropic_content_block(mut block: Value) -> Option<ContentBlock> {
    let block_type = block.get("type")?.as_str()?.to_string();
    match block_type.as_str() {
        "text" => Some(ContentBlock::Text {
            text: block.get("text")?.as_str()?.to_string(),
        }),
        "tool_use" => {
            let id = block.get("id")?.as_str()?.to_string();
            let name = block.get("name")?.as_str()?.to_string();
            let input = if let Some(partial) =
                block.get("_partial_json").and_then(|value| value.as_str())
            {
                serde_json::from_str(partial).unwrap_or(Value::Object(serde_json::Map::new()))
            } else {
                block
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()))
            };
            if let Some(object) = block.as_object_mut() {
                object.remove("_partial_json");
            }
            Some(ContentBlock::ToolUse { id, name, input })
        }
        "thinking" => Some(ContentBlock::Thinking {
            thinking: block.get("thinking")?.as_str()?.to_string(),
            signature: block
                .get("signature")
                .and_then(|value| value.as_str())
                .map(String::from),
        }),
        "image" => {
            let source = block.get("source")?;
            Some(ContentBlock::Image {
                source: ImageContentSource {
                    source_type: source.get("type")?.as_str()?.to_string(),
                    media_type: source.get("media_type")?.as_str()?.to_string(),
                    data: source.get("data")?.as_str()?.to_string(),
                },
            })
        }
        _ => None,
    }
}
