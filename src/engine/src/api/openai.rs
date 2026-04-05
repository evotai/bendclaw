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
use crate::types::ContentBlock;
use crate::types::Message;
use crate::types::MessageRole;
use crate::types::Usage;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const PROVIDER_NAME: &str = "openai";

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    max_tokens: u64,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream_options: Option<OpenAIStreamOptions>,
}

#[derive(Debug, Serialize)]
struct OpenAIStreamOptions {
    include_usage: bool,
}

#[derive(Debug, Serialize)]
struct OpenAIMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OpenAIToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct OpenAITool {
    #[serde(rename = "type")]
    tool_type: String,
    function: OpenAIFunction,
}

#[derive(Debug, Serialize)]
struct OpenAIFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct OpenAIToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OpenAIFunctionCall,
}

#[derive(Debug, Clone, Default, Serialize, serde::Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
}

#[derive(Debug, Clone, Default)]
struct PendingToolCall {
    id: String,
    name: String,
    arguments: String,
    started_emitted: bool,
}

/// OpenAI Chat Completions API provider.
pub struct OpenAIProvider {
    client: Client,
    api_key: String,
    base_url: String,
    custom_headers: HashMap<String, String>,
}

impl OpenAIProvider {
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

    fn chat_completions_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if base.ends_with("/v1") {
            format!("{base}/chat/completions")
        } else {
            format!("{base}/v1/chat/completions")
        }
    }

    fn build_body(&self, request: ProviderRequest<'_>) -> OpenAIRequest {
        let mut openai_messages = Vec::new();

        if let Some(system_blocks) = &request.system {
            let system_text = system_blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");

            if !system_text.is_empty() {
                openai_messages.push(OpenAIMessage {
                    role: "system".to_string(),
                    content: Some(Value::String(system_text)),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        for message in request.messages {
            match message.role {
                MessageRole::User => {
                    let tool_results: Vec<&ContentBlock> = message
                        .content
                        .iter()
                        .filter(|block| matches!(block, ContentBlock::ToolResult { .. }))
                        .collect();

                    if !tool_results.is_empty() {
                        for block in tool_results {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } = block
                            {
                                let text = content
                                    .iter()
                                    .filter_map(|item| match item {
                                        crate::types::ToolResultContentBlock::Text { text } => {
                                            Some(text.as_str())
                                        }
                                        _ => None,
                                    })
                                    .collect::<Vec<_>>()
                                    .join("\n");

                                openai_messages.push(OpenAIMessage {
                                    role: "tool".to_string(),
                                    content: Some(Value::String(text)),
                                    tool_calls: None,
                                    tool_call_id: Some(tool_use_id.clone()),
                                });
                            }
                        }
                    } else {
                        openai_messages.push(OpenAIMessage {
                            role: "user".to_string(),
                            content: Some(Value::String(crate::types::extract_text(message))),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                }
                MessageRole::Assistant => {
                    let text = crate::types::extract_text(message);
                    let tool_calls: Vec<OpenAIToolCall> = message
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::ToolUse { id, name, input } => Some(OpenAIToolCall {
                                id: id.clone(),
                                call_type: "function".to_string(),
                                function: OpenAIFunctionCall {
                                    name: name.clone(),
                                    arguments: serde_json::to_string(input).unwrap_or_default(),
                                },
                            }),
                            _ => None,
                        })
                        .collect();

                    openai_messages.push(OpenAIMessage {
                        role: "assistant".to_string(),
                        content: if text.is_empty() {
                            None
                        } else {
                            Some(Value::String(text))
                        },
                        tool_calls: if tool_calls.is_empty() {
                            None
                        } else {
                            Some(tool_calls)
                        },
                        tool_call_id: None,
                    });
                }
            }
        }

        let tools = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(|tool| OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunction {
                        name: tool.name,
                        description: tool.description,
                        parameters: tool.input_schema,
                    },
                })
                .collect::<Vec<_>>()
        });

        OpenAIRequest {
            model: request.model.to_string(),
            max_tokens: request.max_tokens,
            messages: openai_messages,
            tools: tools.filter(|tools| !tools.is_empty()),
            stream: true,
            stream_options: Some(OpenAIStreamOptions {
                include_usage: true,
            }),
        }
    }

    fn request_builder(&self, body: &OpenAIRequest) -> reqwest::RequestBuilder {
        let mut builder = self
            .client
            .post(self.chat_completions_url())
            .header("authorization", format!("Bearer {}", self.api_key))
            .header("content-type", "application/json");

        for (key, value) in &self.custom_headers {
            builder = builder.header(key, value);
        }

        builder.json(body)
    }

    async fn send_request(&self, body: &OpenAIRequest) -> Result<reqwest::Response, ApiError> {
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
impl LLMProvider for OpenAIProvider {
    fn api_type(&self) -> ApiType {
        ApiType::OpenAICompletions
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
                stream_openai_response(response, writer.clone(), &model_for_task).await
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

async fn stream_openai_response(
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
    let mut tool_calls: HashMap<usize, PendingToolCall> = HashMap::new();
    let mut saw_valid_event = false;
    let mut stop_reason: Option<String> = None;

    while let Some(event) = event_stream.next().await {
        let event = match event {
            Ok(event) => event,
            Err(error) => {
                if !saw_valid_event {
                    if let Some(provider_response) =
                        fallback_from_raw_body(&raw_body, parse_openai_raw_body)?
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

        if let Some(usage) = payload.get("usage") {
            writer.usage(parse_openai_usage(Some(usage))).await;
        }

        if let Some(choices) = payload.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    stop_reason = Some(map_openai_stop_reason(reason));
                }

                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    writer.text(content).await;
                }

                if let Some(tool_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tool_delta in tool_deltas {
                        let index =
                            tool_delta.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tool_calls.entry(index).or_default();

                        if let Some(id) = tool_delta.get("id").and_then(Value::as_str) {
                            entry.id = id.to_string();
                        }

                        if let Some(function) = tool_delta.get("function") {
                            if let Some(name) = function.get("name").and_then(Value::as_str) {
                                entry.name = name.to_string();
                            }
                            if let Some(arguments) =
                                function.get("arguments").and_then(Value::as_str)
                            {
                                if !arguments.is_empty() {
                                    writer.tool_delta(index, arguments).await;
                                    entry.arguments.push_str(arguments);
                                }
                            }
                        }

                        if !entry.started_emitted && !entry.id.is_empty() && !entry.name.is_empty()
                        {
                            writer.tool_start(index, &entry.id, &entry.name).await;
                            entry.started_emitted = true;
                        }
                    }
                }
            }
        }
    }

    if !saw_valid_event {
        let provider_response = fallback_from_raw_body(&raw_body, parse_openai_raw_body)?
            .ok_or_else(|| {
                ApiError::StreamError("OpenAI upstream returned empty stream".to_string())
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

    let mut sorted_tool_calls: Vec<(usize, PendingToolCall)> = tool_calls.into_iter().collect();
    sorted_tool_calls.sort_by_key(|(index, _)| *index);

    for (index, tool_call) in sorted_tool_calls {
        let id = if tool_call.id.is_empty() {
            format!("call_{}", uuid::Uuid::new_v4())
        } else {
            tool_call.id
        };

        if !tool_call.started_emitted && !tool_call.name.is_empty() {
            writer.tool_start(index, &id, &tool_call.name).await;
        }

        writer
            .tool_end(index, id, tool_call.name, tool_call.arguments)
            .await;
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

fn parse_openai_fallback(body: &str) -> Result<ProviderResponse, ApiError> {
    let value = response::parse_json_body(body, "OpenAI")?;
    if let Some(error) = response::json_stream_error(&value, "OpenAI") {
        return Err(error);
    }
    parse_openai_response(&value)
}

fn parse_openai_raw_body(body: &str) -> Result<ProviderResponse, ApiError> {
    if body
        .lines()
        .any(|line| line.trim_start().starts_with("data: "))
    {
        return parse_openai_sse_body(body);
    }

    parse_openai_fallback(body)
}

fn parse_openai_sse_body(body: &str) -> Result<ProviderResponse, ApiError> {
    let mut text_content = String::new();
    let mut tool_calls: HashMap<usize, OpenAIToolCall> = HashMap::new();
    let mut usage = Usage::default();
    let mut stop_reason: Option<String> = None;
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

        if let Some(usage_value) = payload.get("usage") {
            usage = parse_openai_usage(Some(usage_value));
        }

        if let Some(choices) = payload.get("choices").and_then(Value::as_array) {
            for choice in choices {
                if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                    stop_reason = Some(map_openai_stop_reason(reason));
                }

                let Some(delta) = choice.get("delta") else {
                    continue;
                };

                if let Some(content) = delta.get("content").and_then(Value::as_str) {
                    text_content.push_str(content);
                }

                if let Some(tool_deltas) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tool_delta in tool_deltas {
                        let index =
                            tool_delta.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        let entry = tool_calls.entry(index).or_insert_with(|| OpenAIToolCall {
                            id: String::new(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall::default(),
                        });

                        if let Some(id) = tool_delta.get("id").and_then(Value::as_str) {
                            entry.id = id.to_string();
                        }

                        if let Some(function) = tool_delta.get("function") {
                            if let Some(name) = function.get("name").and_then(Value::as_str) {
                                entry.function.name = name.to_string();
                            }
                            if let Some(arguments) =
                                function.get("arguments").and_then(Value::as_str)
                            {
                                entry.function.arguments.push_str(arguments);
                            }
                        }
                    }
                }
            }
        }
    }

    if !saw_valid_sse_event {
        return parse_openai_fallback(body);
    }

    let mut content_blocks = Vec::new();
    if !text_content.is_empty() {
        content_blocks.push(ContentBlock::Text { text: text_content });
    }

    let mut sorted_calls: Vec<(usize, OpenAIToolCall)> = tool_calls.into_iter().collect();
    sorted_calls.sort_by_key(|(index, _)| *index);
    for (_, tool_call) in sorted_calls {
        let input = serde_json::from_str(&tool_call.function.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));
        content_blocks.push(ContentBlock::ToolUse {
            id: if tool_call.id.is_empty() {
                format!("call_{}", uuid::Uuid::new_v4())
            } else {
                tool_call.id
            },
            name: tool_call.function.name,
            input,
        });
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

fn parse_openai_response(value: &Value) -> Result<ProviderResponse, ApiError> {
    let choice = value
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .ok_or_else(|| {
            ApiError::StreamError("OpenAI upstream returned JSON body without choices".to_string())
        })?;

    let message = choice.get("message").ok_or_else(|| {
        ApiError::StreamError("OpenAI upstream returned JSON body without message".to_string())
    })?;

    let mut content_blocks = Vec::new();

    if let Some(content) = message.get("content") {
        if let Some(text) = content.as_str() {
            if !text.is_empty() {
                content_blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        } else if let Some(parts) = content.as_array() {
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    if !text.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: text.to_string(),
                        });
                    }
                }
            }
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array) {
        for tool_call in tool_calls {
            let id = tool_call
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let Some(function) = tool_call.get("function") else {
                continue;
            };
            let name = function
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let input = parse_openai_tool_arguments(function.get("arguments"));

            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }
    }

    let usage = parse_openai_usage(value.get("usage"));
    let stop_reason = choice
        .get("finish_reason")
        .and_then(Value::as_str)
        .map(map_openai_stop_reason);

    Ok(ProviderResponse {
        message: Message {
            role: MessageRole::Assistant,
            content: content_blocks,
        },
        usage,
        stop_reason,
    })
}

fn parse_openai_usage(value: Option<&Value>) -> Usage {
    let mut usage = Usage::default();

    if let Some(value) = value {
        if let Some(prompt_tokens) = value.get("prompt_tokens").and_then(Value::as_u64) {
            usage.input_tokens = prompt_tokens;
        }
        if let Some(completion_tokens) = value.get("completion_tokens").and_then(Value::as_u64) {
            usage.output_tokens = completion_tokens;
        }
    }

    usage
}

fn map_openai_stop_reason(reason: &str) -> String {
    match reason {
        "stop" => "end_turn".to_string(),
        "tool_calls" => "tool_use".to_string(),
        "length" => "max_tokens".to_string(),
        other => other.to_string(),
    }
}

fn parse_openai_tool_arguments(arguments: Option<&Value>) -> Value {
    match arguments {
        Some(Value::String(arguments)) => match serde_json::from_str(arguments) {
            Ok(value) => value,
            Err(_) => Value::Object(serde_json::Map::new()),
        },
        Some(value) if value.is_object() || value.is_array() => value.clone(),
        _ => Value::Object(serde_json::Map::new()),
    }
}
