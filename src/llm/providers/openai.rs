use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tokio_stream::StreamExt;

use super::common;
use crate::llm::http_adapter;
use crate::llm::message::ChatMessage;
use crate::llm::message::Content;
use crate::llm::message::ToolCall;
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
use crate::types::http;
use crate::types::ErrorCode;
use crate::types::Result;

/// OpenAI-compatible provider. Works with OpenAI, DeepSeek, Groq, OpenRouter, etc.
pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: String,
}

impl OpenAIProvider {
    pub fn new(base_url: &str, api_key: &str) -> crate::types::Result<Self> {
        let client = common::build_http_client()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
        })
    }

    fn build_body(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
        stream: bool,
    ) -> Result<serde_json::Value> {
        let mut body = json!({
            "model": model,
            "messages": serialize_messages(messages),
            "temperature": temperature,
        });
        if stream {
            body["stream"] = json!(true);
            body["stream_options"] = json!({ "include_usage": true });
        }
        if !tools.is_empty() {
            body["tools"] = serde_json::to_value(tools)?;
            body["tool_choice"] = json!("auto");
        }
        Ok(body)
    }
}

#[async_trait]
impl LLMProvider for OpenAIProvider {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> Result<LLMResponse> {
        common::log_request("openai", model, messages.len());
        let body = self.build_body(model, messages, tools, temperature, false)?;
        let url = format!("{}/chat/completions", self.base_url);
        let resp = http::send(
            self.client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .header("Content-Type", "application/json")
                .json(&body),
            http::HttpRequestContext::new("llm", "request_send")
                .with_endpoint("openai")
                .with_model(model.to_string())
                .with_url(url.clone()),
        )
        .await
        .map_err(http_adapter::to_llm_error)?;

        let status = resp.status();
        let headers = resp.headers().clone();
        let request_id = response_request_id(&headers);
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            common::log_api_error(
                "openai",
                model,
                &self.base_url,
                &self.api_key,
                status,
                &request_id,
                &text,
            );
            return Err(common::classify_api_error("OpenAI", status, &text));
        }

        let data: serde_json::Value = http::read_json(
            resp,
            http::HttpRequestContext::new("llm", "decode_response")
                .with_endpoint("openai")
                .with_model(model.to_string())
                .with_url(url.clone()),
        )
        .await
        .map_err(http_adapter::to_llm_error)?;

        let result = parse_response(&data)?;

        Ok(result)
    }

    fn chat_stream(
        &self,
        model: &str,
        messages: &[ChatMessage],
        tools: &[ToolSchema],
        temperature: f64,
    ) -> ResponseStream {
        let body = match self.build_body(model, messages, tools, temperature, true) {
            Ok(b) => b,
            Err(e) => return ResponseStream::from_error(e),
        };
        let (writer, stream) = ResponseStream::channel(64);
        let url = format!("{}/chat/completions", self.base_url);
        let client = self.client.clone();
        let api_key = self.api_key.clone();
        let model_owned = model.to_string();

        crate::types::spawn_fire_and_forget("openai_stream_driver", async move {
            let request_ctx = http::HttpRequestContext::new("llm", "stream_open")
                .with_endpoint("openai")
                .with_model(model_owned.clone())
                .with_url(url.clone());
            if let Err(msg) = drive_stream(&client, &api_key, &body, &writer, request_ctx).await {
                common::log_stream_failed("openai", &model_owned, &msg);
                writer.error(msg).await;
            }
        });

        stream
    }
}

/// Drive the SSE stream, pushing events into the writer.
#[allow(clippy::too_many_arguments)]
async fn drive_stream(
    client: &Client,
    api_key: &str,
    body: &serde_json::Value,
    writer: &StreamWriter,
    request_ctx: http::HttpRequestContext,
) -> std::result::Result<(), String> {
    let resp = http::open_stream(
        client
            .post(&request_ctx.url)
            .header("Authorization", format!("Bearer {api_key}"))
            .header("Content-Type", "application/json")
            .json(body),
        request_ctx.clone(),
    )
    .await
    .map_err(http_adapter::to_stream_error)?;

    let resp_headers = resp.headers().clone();
    let request_id = response_request_id(&resp_headers);

    if !resp.status().is_success() {
        let error = common::read_stream_error(resp, "openai", &request_ctx).await;
        common::log_stream_api_error(
            "openai",
            &request_ctx,
            &request_id,
            error.status,
            error.text.len(),
        );

        return Err(common::api_error_message(
            "OpenAI",
            error.status,
            &error.text,
        ));
    }

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !common::is_streaming_content_type(&content_type) {
        let fallback = common::read_stream_fallback_body(resp, "openai", &request_ctx).await?;
        let data = fallback.data;
        let parsed = parse_response(&data)
            .map_err(|e| format!("non-streaming response parse failed: {e}"))?;

        common::log_stream_fallback(
            "openai",
            request_ctx.model.as_deref().unwrap_or(""),
            Some(&request_id),
            &content_type,
        );

        emit_response(
            writer,
            parsed,
            request_ctx.model.as_deref().unwrap_or_default(),
        )
        .await;
        return Ok(());
    }

    let mut parser = SseParser::new();
    let mut tool_calls = ToolCallAccumulator::new();
    let mut finish_reason = String::from("stop");
    let mut saw_stream_event = false;

    let mut byte_stream = resp.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| {
            http_adapter::to_stream_error(http::stream_read_error(
                e,
                http::HttpRequestContext::new("llm", "stream_read")
                    .with_endpoint("openai")
                    .with_model(request_ctx.model.clone().unwrap_or_default())
                    .with_url(request_ctx.url.clone()),
            ))
        })?;

        for data in parser.feed(&chunk) {
            saw_stream_event = true;
            match data {
                SseData::Done => {
                    for tc in tool_calls.drain() {
                        writer
                            .tool_end(tc.index, &tc.id, &tc.name, &tc.arguments)
                            .await;
                    }
                    common::stream_done(
                        writer,
                        &finish_reason,
                        "openai",
                        request_ctx.model.clone(),
                    )
                    .await;
                    return Ok(());
                }
                SseData::Json(parsed) => {
                    if let Some(message) = stream_error_message(&parsed) {
                        common::log_stream_event_error(
                            "openai",
                            request_ctx.model.as_deref().unwrap_or(""),
                            &request_id,
                            &message,
                            &parsed.to_string(),
                        );
                        return Err(message);
                    }

                    // Usage (may come in a separate chunk with stream_options)
                    if let Some(u) = parsed.get("usage") {
                        writer.usage(TokenUsage::from_openai_json(u)).await;
                    }

                    let choice = match parsed
                        .get("choices")
                        .and_then(|c| c.as_array())
                        .and_then(|a| a.first())
                    {
                        Some(c) => c,
                        None => continue,
                    };

                    if let Some(reason) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                        finish_reason = reason.to_string();
                    }

                    let delta = match choice.get("delta") {
                        Some(d) => d,
                        None => continue,
                    };

                    // Text content
                    if let Some(text) = delta.get("content").and_then(|c| c.as_str()) {
                        if !text.is_empty() {
                            writer.text(text).await;
                        }
                    }

                    // Tool calls
                    if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                        for tc in tcs {
                            let index =
                                tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                            let slot = tool_calls.get_or_create(index);

                            if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                                slot.id = id.to_string();
                            }

                            if let Some(func) = tc.get("function") {
                                if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                    slot.name = name.to_string();
                                    writer.tool_start(index, &slot.id, name).await;
                                }
                                if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                    slot.arguments.push_str(args);
                                    writer.tool_delta(index, args).await;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let trailing_body = parser.take_remaining();
    if !saw_stream_event && !trailing_body.trim().is_empty() {
        return emit_body_fallback(
            writer,
            request_ctx.model.as_deref().unwrap_or_default(),
            &request_id,
            &content_type,
            &trailing_body,
        )
        .await;
    }

    // Emit tool_end for any accumulated tool calls
    for tc in tool_calls.drain() {
        writer
            .tool_end(tc.index, &tc.id, &tc.name, &tc.arguments)
            .await;
    }

    common::stream_done(writer, &finish_reason, "openai", request_ctx.model.clone()).await;
    Ok(())
}

async fn emit_response(writer: &StreamWriter, response: LLMResponse, model: &str) {
    if let Some(usage) = response.usage {
        writer.usage(usage).await;
    }

    if let Some(text) = response.content {
        if !text.is_empty() {
            writer.text(&text).await;
        }
    }

    for (index, tool_call) in response.tool_calls.iter().enumerate() {
        writer
            .tool_start(index, &tool_call.id, &tool_call.name)
            .await;
        if !tool_call.arguments.is_empty() {
            writer.tool_delta(index, &tool_call.arguments).await;
        }
        writer
            .tool_end(index, &tool_call.id, &tool_call.name, &tool_call.arguments)
            .await;
    }

    let finish_reason = response.finish_reason.as_deref().unwrap_or("stop");
    let model = response.model.unwrap_or_else(|| model.to_string());
    common::stream_done(writer, finish_reason, "openai", Some(model)).await;
}

async fn emit_body_fallback(
    writer: &StreamWriter,
    model: &str,
    request_id: &str,
    content_type: &str,
    body: &str,
) -> std::result::Result<(), String> {
    let data: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("stream trailing body parse failed: {e}"))?;
    let parsed =
        parse_response(&data).map_err(|e| format!("stream trailing body parse failed: {e}"))?;

    common::log_stream_body_fallback("openai", model, request_id, content_type, body);

    emit_response(writer, parsed, model).await;
    Ok(())
}

fn serialize_messages(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .map(|m| {
            let mut obj = json!({ "role": m.role });

            // Content: string for single text, array for multimodal
            match m.content.as_slice() {
                [] => {
                    obj["content"] = json!(null);
                }
                [Content::Text { text }] => {
                    obj["content"] = json!(text);
                }
                parts => {
                    let blocks: Vec<serde_json::Value> = parts
                        .iter()
                        .map(|c| match c {
                            Content::Text { text } => json!({ "type": "text", "text": text }),
                            Content::Image { data, mime_type } => json!({
                                "type": "image_url",
                                "image_url": { "url": format!("data:{mime_type};base64,{data}") }
                            }),
                        })
                        .collect();
                    obj["content"] = json!(blocks);
                }
            }

            if !m.tool_calls.is_empty() {
                let tc: Vec<serde_json::Value> = m
                    .tool_calls
                    .iter()
                    .map(|tc| {
                        json!({
                            "id": tc.id,
                            "type": "function",
                            "function": {
                                "name": tc.name,
                                "arguments": tc.arguments,
                            }
                        })
                    })
                    .collect();
                obj["tool_calls"] = json!(tc);
            }

            if let Some(ref id) = m.tool_call_id {
                obj["tool_call_id"] = json!(id);
            }

            obj
        })
        .collect()
}

fn parse_response(data: &serde_json::Value) -> Result<LLMResponse> {
    use crate::types::OptionExt;

    if let Some(message) = stream_error_message(data) {
        return Err(ErrorCode::llm_request(message));
    }

    let choice = data
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .ok_or_error(|| ErrorCode::llm_request("no choices in response"))?;

    let message = choice
        .get("message")
        .ok_or_error(|| ErrorCode::llm_request("no message in choice"))?;

    let content = message
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from);

    let finish_reason = choice
        .get("finish_reason")
        .and_then(|f| f.as_str())
        .map(String::from);

    let tool_calls = message
        .get("tool_calls")
        .and_then(|tc| tc.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|tc| {
                    let id = tc.get("id")?.as_str()?.to_string();
                    let func = tc.get("function")?;
                    let name = func.get("name")?.as_str()?.to_string();
                    let arguments = func
                        .get("arguments")
                        .map(|a| {
                            if a.is_string() {
                                a.as_str().unwrap_or("{}").to_string()
                            } else {
                                a.to_string()
                            }
                        })
                        .unwrap_or_else(|| "{}".to_string());
                    Some(ToolCall {
                        id,
                        name,
                        arguments,
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    let usage = data.get("usage").map(TokenUsage::from_openai_json);

    let model = data.get("model").and_then(|m| m.as_str()).map(String::from);

    Ok(LLMResponse {
        content,
        tool_calls,
        finish_reason,
        usage,
        model,
    })
}

fn stream_error_message(data: &serde_json::Value) -> Option<String> {
    let error = data.get("error")?;
    let message = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let error_type = error.get("type").and_then(|v| v.as_str()).unwrap_or("");

    if message.is_empty() && error_type.is_empty() {
        return None;
    }

    if error_type.is_empty() {
        return Some(format!("OpenAI stream error: {message}"));
    }
    if message.is_empty() {
        return Some(format!("OpenAI stream error ({error_type})"));
    }

    Some(format!("OpenAI stream error ({error_type}): {message}"))
}
