use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tokio_stream::StreamExt;
use tracing;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::llm::message::CacheControl;
use crate::llm::message::ChatMessage;
use crate::llm::message::Content;
use crate::llm::message::Role;
use crate::llm::message::ToolCall;
use crate::llm::provider::mask_api_key;
use crate::llm::provider::response_headers_value;
use crate::llm::provider::response_request_id;
use crate::llm::provider::LLMProvider;
use crate::llm::provider::LLMResponse;
use crate::llm::sse::SseData;
use crate::llm::sse::SseParser;
use crate::llm::stream::ResponseStream;
use crate::llm::stream::StreamWriter;
use crate::llm::stream::ToolCallAccumulator;
use crate::llm::tool::ToolSchema;
use crate::llm::usage::TokenUsage;

const MAX_TOKENS: u32 = 8192;
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    client: Client,
    base_url: String,
    api_key: String,
}

impl AnthropicProvider {
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            client: Client::builder()
                .connect_timeout(std::time::Duration::from_secs(30))
                .timeout(std::time::Duration::from_secs(300))
                .build()
                .expect("failed to build HTTP client"),
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        }
    }

    fn build_body(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
        stream: bool,
    ) -> (serde_json::Value, String) {
        let (system_prompt, api_messages) = to_anthropic_messages(messages);

        let mut body = json!({
            "model": model,
            "messages": api_messages,
            "max_tokens": MAX_TOKENS,
            "temperature": temperature,
        });

        if stream {
            body["stream"] = json!(true);
        }

        if let Some(sys) = system_prompt {
            body["system"] = sys;
        }

        if !tools.is_empty() {
            body["tools"] = json!(to_anthropic_tools(tools));
        }

        let url = format!("{}/messages", self.base_url);
        (body, url)
    }
}

#[async_trait]
impl LLMProvider for AnthropicProvider {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse> {
        tracing::info!(
            provider = "anthropic",
            model,
            msg_count = messages.len(),
            "llm chat request started"
        );
        let (body, url) = self.build_body(model, messages, tools, temperature, false);

        let resp = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", ANTHROPIC_VERSION)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    provider = "anthropic",
                    model,
                    base_url = %self.base_url,
                    api_key = %mask_api_key(&self.api_key),
                    error = %e,
                    "llm chat request failed"
                );
                ErrorCode::llm_request(format!("request failed: {e}"))
            })?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let request_id = response_request_id(&headers);
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            tracing::error!(
                provider = "anthropic",
                model,
                base_url = %self.base_url,
                api_key = %mask_api_key(&self.api_key),
                status = %status,
                request_id = %request_id,
                headers = %response_headers_value(&headers),
                response = %truncate_for_log(&text),
                "llm chat api error"
            );
            return Err(ErrorCode::llm_request(format!(
                "Anthropic API error {status}: {text}"
            )));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| {
            tracing::error!(
                provider = "anthropic",
                model,
                base_url = %self.base_url,
                api_key = %mask_api_key(&self.api_key),
                request_id = %request_id,
                headers = %response_headers_value(&headers),
                error = %e,
                "llm chat response parse failed"
            );
            ErrorCode::llm_request(format!("response parse failed: {e}"))
        })?;

        let result = parse_response(&data)?;
        tracing::info!(
            provider = "anthropic",
            model,
            request_id = %request_id,
            headers = %response_headers_value(&headers),
            prompt_tokens = result.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0),
            completion_tokens = result.usage.as_ref().map(|u| u.completion_tokens).unwrap_or(0),
            finish_reason = ?result.finish_reason,
            "llm chat request completed"
        );
        Ok(result)
    }

    fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream {
        let (writer, stream) = ResponseStream::channel(64);
        let (body, url) = self.build_body(model, messages, tools, temperature, true);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let base_url = self.base_url.clone();
        let masked_api_key = mask_api_key(&self.api_key);
        let model_owned = model.to_string();

        tokio::spawn(async move {
            if let Err(msg) = drive_stream(
                &client,
                &url,
                &api_key,
                &base_url,
                &masked_api_key,
                &body,
                &writer,
                &model_owned,
            )
            .await
            {
                tracing::error!(
                    provider = "anthropic",
                    model = %model_owned,
                    base_url = %base_url,
                    api_key = %masked_api_key,
                    error = %msg,
                    "llm stream failed"
                );
                writer.error(msg).await;
            }
        });

        stream
    }
}

#[allow(clippy::too_many_arguments)]
async fn drive_stream(
    client: &Client,
    url: &str,
    api_key: &str,
    base_url: &str,
    masked_api_key: &str,
    body: &serde_json::Value,
    writer: &StreamWriter,
    model: &str,
) -> std::result::Result<(), String> {
    tracing::info!(
        provider = "anthropic",
        model = %model,
        url = %url,
        api_key = %masked_api_key,
        body_bytes = body.to_string().len(),
        "llm stream request"
    );

    let resp = client
        .post(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let headers = resp.headers().clone();
        let request_id = response_request_id(&headers);
        let text = resp.text().await.unwrap_or_default();
        tracing::error!(
            provider = "anthropic",
            model = %model,
            base_url = %base_url,
            api_key = %masked_api_key,
            status = %status,
            request_id = %request_id,
            headers = %response_headers_value(&headers),
            response_bytes = text.len(),
            "llm stream api error"
        );
        tracing::debug!(provider = "anthropic", model = %model, response = %truncate_for_log(&text), "llm stream api error body");
        return Err(format!("Anthropic API error {status}: {text}"));
    }

    // Fallback: if backend doesn't support streaming, parse as JSON response
    let headers = resp.headers().clone();
    let request_id = response_request_id(&headers);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    tracing::info!(
        provider = "anthropic",
        model = %model,
        request_id = %request_id,
        headers = %response_headers_value(&headers),
        status = %resp.status(),
        content_type = %content_type,
        "llm stream response"
    );

    if !content_type.contains("stream") && !content_type.contains("event-stream") {
        let body = resp
            .text()
            .await
            .map_err(|e| format!("body read error: {e}"))?;
        let data: serde_json::Value = serde_json::from_str(&body)
            .map_err(|e| format!("non-streaming response parse failed: {e}"))?;

        tracing::warn!(
            provider = "anthropic",
            content_type,
            "backend returned non-streaming response, falling back to JSON parse"
        );

        if let Some(blocks) = data.get("content").and_then(|c| c.as_array()) {
            for (i, block) in blocks.iter().enumerate() {
                match block.get("type").and_then(|t| t.as_str()) {
                    Some("text") => {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            writer.text(text).await;
                        }
                    }
                    Some("tool_use") => {
                        let id = block
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = block
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let arguments = block
                            .get("input")
                            .map(|v| serde_json::to_string(v).unwrap_or_else(|_| "{}".into()))
                            .unwrap_or_else(|| "{}".into());
                        writer.tool_start(i, &id, &name).await;
                        writer.tool_end(i, &id, &name, &arguments).await;
                    }
                    _ => {}
                }
            }
        }
        if let Some(u) = data.get("usage") {
            writer.usage(TokenUsage::from_anthropic_json(u)).await;
        }
        let reason = data
            .get("stop_reason")
            .and_then(|s| s.as_str())
            .unwrap_or("end_turn");
        writer
            .done_with_provider(
                reason,
                Some("anthropic".to_string()),
                Some(model.to_string()),
            )
            .await;
        return Ok(());
    }

    let mut parser = SseParser::new();
    let mut tool_calls = ToolCallAccumulator::new();
    let mut current_block_index: usize = 0;
    let mut finish_reason = String::from("end_turn");

    let mut byte_stream = resp.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream read error: {e}"))?;

        for data in parser.feed(&chunk) {
            let parsed = match data {
                SseData::Json(v) => v,
                SseData::Done => {
                    writer
                        .done_with_provider(
                            &finish_reason,
                            Some("anthropic".to_string()),
                            Some(model.to_string()),
                        )
                        .await;
                    return Ok(());
                }
            };

            let event_type = parsed.get("type").and_then(|t| t.as_str()).unwrap_or("");

            match event_type {
                "message_start" => {
                    if let Some(u) = parsed.get("message").and_then(|m| m.get("usage")) {
                        writer.usage(TokenUsage::from_anthropic_json(u)).await;
                    }
                }

                "content_block_start" => {
                    current_block_index =
                        parsed.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                    if let Some(block) = parsed.get("content_block") {
                        if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                            let tc = tool_calls.get_or_create(current_block_index);
                            tc.id = block
                                .get("id")
                                .and_then(|i| i.as_str())
                                .ok_or_else(|| {
                                    "anthropic stream: tool_use block missing id".to_string()
                                })?
                                .to_string();
                            tc.name = block
                                .get("name")
                                .and_then(|n| n.as_str())
                                .ok_or_else(|| {
                                    "anthropic stream: tool_use block missing name".to_string()
                                })?
                                .to_string();
                            writer
                                .tool_start(current_block_index, &tc.id, &tc.name)
                                .await;
                        }
                    }
                }

                "content_block_delta" => {
                    if let Some(delta) = parsed.get("delta") {
                        match delta.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                            "text_delta" => {
                                if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                    writer.text(text).await;
                                }
                            }
                            "thinking_delta" => {
                                if let Some(text) = delta.get("thinking").and_then(|t| t.as_str()) {
                                    writer.thinking(text).await;
                                }
                            }
                            "input_json_delta" => {
                                if let Some(json_str) =
                                    delta.get("partial_json").and_then(|j| j.as_str())
                                {
                                    let tc = tool_calls.get_or_create(current_block_index);
                                    tc.arguments.push_str(json_str);
                                    writer.tool_delta(current_block_index, json_str).await;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                "content_block_stop" => {
                    let stop_index =
                        parsed.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                    if let Some(tc) = tool_calls.find(stop_index) {
                        writer
                            .tool_end(tc.index, &tc.id, &tc.name, &tc.arguments)
                            .await;
                    }
                }

                "message_delta" => {
                    if let Some(delta) = parsed.get("delta") {
                        if let Some(reason) = delta.get("stop_reason").and_then(|s| s.as_str()) {
                            finish_reason = reason.to_string();
                        }
                    }
                    if let Some(u) = parsed.get("usage") {
                        writer.usage(TokenUsage::from_anthropic_json(u)).await;
                    }
                }

                "message_stop" => {
                    writer
                        .done_with_provider(
                            &finish_reason,
                            Some("anthropic".to_string()),
                            Some(model.to_string()),
                        )
                        .await;
                    return Ok(());
                }

                "error" => {
                    let msg = parsed
                        .get("error")
                        .and_then(|e| e.get("message"))
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown error");
                    return Err(msg.to_string());
                }

                _ => {}
            }
        }
    }

    // Stream ended without message_stop — still signal done
    writer
        .done_with_provider(
            &finish_reason,
            Some("anthropic".to_string()),
            Some(model.to_string()),
        )
        .await;
    Ok(())
}

fn to_anthropic_messages(
    messages: &[ChatMessage],
) -> (Option<serde_json::Value>, Vec<serde_json::Value>) {
    let mut system_prompt: Option<serde_json::Value> = None;
    let mut result = Vec::new();

    for msg in messages {
        let cache = &msg.cache_control;

        match msg.role {
            Role::System => {
                if cache.is_some() {
                    system_prompt = Some(json!([{
                        "type": "text",
                        "text": msg.text(),
                        "cache_control": { "type": "ephemeral" },
                    }]));
                } else {
                    system_prompt = Some(json!(msg.text()));
                }
            }
            Role::User => {
                let content = serialize_content_with_cache(&msg.content, cache);
                result.push(json!({ "role": "user", "content": content }));
            }
            Role::Assistant => {
                let mut blocks: Vec<serde_json::Value> = Vec::new();
                let text = msg.text();
                if !text.is_empty() {
                    blocks.push(json!({ "type": "text", "text": text }));
                }
                for tc in &msg.tool_calls {
                    let input: serde_json::Value =
                        serde_json::from_str(&tc.arguments).unwrap_or(json!({}));
                    blocks.push(json!({
                        "type": "tool_use",
                        "id": tc.id,
                        "name": tc.name,
                        "input": input,
                    }));
                }
                if blocks.is_empty() {
                    blocks.push(json!({ "type": "text", "text": "" }));
                }
                if cache.is_some() {
                    if let Some(last) = blocks.last_mut() {
                        last["cache_control"] = json!({ "type": "ephemeral" });
                    }
                }
                result.push(json!({ "role": "assistant", "content": blocks }));
            }
            Role::Tool => {
                let mut block = json!({
                    "type": "tool_result",
                    "tool_use_id": msg.tool_call_id.as_deref().unwrap_or(""),
                    "content": msg.text(),
                });
                if cache.is_some() {
                    block["cache_control"] = json!({ "type": "ephemeral" });
                }
                result.push(json!({
                    "role": "user",
                    "content": [block],
                }));
            }
        }
    }

    (system_prompt, result)
}

fn serialize_content_with_cache(
    content: &[Content],
    cache: &Option<CacheControl>,
) -> serde_json::Value {
    match content {
        [] => json!(""),
        [Content::Text { text }] if cache.is_none() => json!(text),
        parts => {
            let len = parts.len();
            let blocks: Vec<serde_json::Value> = parts
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    let mut block = match c {
                        Content::Text { text } => json!({ "type": "text", "text": text }),
                        Content::Image { data, mime_type } => json!({
                            "type": "image",
                            "source": {
                                "type": "base64",
                                "media_type": mime_type,
                                "data": data,
                            }
                        }),
                    };
                    if i == len - 1 && cache.is_some() {
                        block["cache_control"] = json!({ "type": "ephemeral" });
                    }
                    block
                })
                .collect();
            json!(blocks)
        }
    }
}

fn to_anthropic_tools(tools: &[ToolSchema]) -> Vec<serde_json::Value> {
    tools
        .iter()
        .map(|t| {
            json!({
                "name": t.function.name,
                "description": t.function.description,
                "input_schema": t.function.parameters,
            })
        })
        .collect()
}

fn truncate_for_log(text: &str) -> &str {
    const MAX_LOG_LEN: usize = 512;
    if text.len() <= MAX_LOG_LEN {
        text
    } else {
        &text[..MAX_LOG_LEN]
    }
}

fn parse_response(data: &serde_json::Value) -> Result<LLMResponse> {
    let content_blocks = data
        .get("content")
        .and_then(|c| c.as_array())
        .cloned()
        .unwrap_or_default();

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in &content_blocks {
        let block_type = block.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match block_type {
            "text" => {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    text_parts.push(text.to_string());
                }
            }
            "tool_use" => {
                let id = block
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("")
                    .to_string();
                let input = block.get("input").cloned().unwrap_or(json!({}));
                let arguments = serde_json::to_string(&input).unwrap_or_else(|_| "{}".into());
                tool_calls.push(ToolCall {
                    id,
                    name,
                    arguments,
                });
            }
            _ => {}
        }
    }

    let content = if text_parts.is_empty() {
        None
    } else {
        Some(text_parts.join(""))
    };

    let finish_reason = data
        .get("stop_reason")
        .and_then(|s| s.as_str())
        .map(|r| match r {
            "tool_use" => "tool_calls".to_string(),
            "end_turn" => "stop".to_string(),
            other => other.to_string(),
        });

    let usage = data.get("usage").map(TokenUsage::from_anthropic_json);

    let model = data.get("model").and_then(|m| m.as_str()).map(String::from);

    Ok(LLMResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
        model,
    })
}
