use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use tokio_stream::StreamExt;
use tracing;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::llm::message::ChatMessage;
use crate::llm::message::Content;
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

/// OpenAI-compatible provider. Works with OpenAI, DeepSeek, Groq, OpenRouter, etc.
pub struct OpenAIProvider {
    client: Client,
    base_url: String,
    api_key: String,
}

impl OpenAIProvider {
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
        tracing::info!(
            provider = "openai",
            model,
            msg_count = messages.len(),
            "llm chat request started"
        );
        let body = self.build_body(model, messages, tools, temperature, false)?;
        let url = format!("{}/chat/completions", self.base_url);

        let resp = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                tracing::error!(
                    provider = "openai",
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
                provider = "openai",
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
                "OpenAI API error {status}: {text}"
            )));
        }

        let data: serde_json::Value = resp.json().await.map_err(|e| {
            tracing::error!(
                provider = "openai",
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
            provider = "openai",
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
        let body = match self.build_body(model, messages, tools, temperature, true) {
            Ok(b) => b,
            Err(e) => return ResponseStream::from_error(e),
        };
        let (writer, stream) = ResponseStream::channel(64);
        let url = format!("{}/chat/completions", self.base_url);
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
                    provider = "openai",
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

/// Drive the SSE stream, pushing events into the writer.
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
        provider = "openai",
        model = %model,
        url = %url,
        api_key = %masked_api_key,
        body_bytes = body.to_string().len(),
        "llm stream request"
    );

    let resp = client
        .post(url)
        .header("Authorization", format!("Bearer {api_key}"))
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
            provider = "openai",
            model = %model,
            base_url = %base_url,
            api_key = %masked_api_key,
            status = %status,
            request_id = %request_id,
            headers = %response_headers_value(&headers),
            response_bytes = text.len(),
            "llm stream api error"
        );
        tracing::debug!(provider = "openai", model = %model, response = %truncate_for_log(&text), "llm stream api error body");
        return Err(format!("OpenAI API error {status}: {text}"));
    }

    let headers = resp.headers().clone();
    let request_id = response_request_id(&headers);

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    tracing::info!(
        provider = "openai",
        model = %model,
        request_id = %request_id,
        headers = %response_headers_value(&headers),
        status = %resp.status(),
        content_type = %content_type,
        "llm stream response"
    );

    let mut parser = SseParser::new();
    let mut tool_calls = ToolCallAccumulator::new();
    let mut finish_reason = String::from("stop");

    let mut byte_stream = resp.bytes_stream();

    while let Some(chunk) = byte_stream.next().await {
        let chunk = chunk.map_err(|e| format!("stream read error: {e}"))?;

        for data in parser.feed(&chunk) {
            match data {
                SseData::Done => {
                    writer
                        .done_with_provider(
                            &finish_reason,
                            Some("openai".to_string()),
                            Some(model.to_string()),
                        )
                        .await;
                    return Ok(());
                }
                SseData::Json(parsed) => {
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

    // Emit tool_end for any accumulated tool calls
    for tc in tool_calls.drain() {
        writer
            .tool_end(tc.index, &tc.id, &tc.name, &tc.arguments)
            .await;
    }

    writer
        .done_with_provider(
            &finish_reason,
            Some("openai".to_string()),
            Some(model.to_string()),
        )
        .await;
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

fn truncate_for_log(text: &str) -> &str {
    const MAX_LOG_LEN: usize = 512;
    if text.len() <= MAX_LOG_LEN {
        text
    } else {
        &text[..MAX_LOG_LEN]
    }
}

fn parse_response(data: &serde_json::Value) -> Result<LLMResponse> {
    use crate::base::OptionExt;

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
