//! OpenAI Responses API provider.
//!
//! This is the newer OpenAI API that uses a different event format
//! from Chat Completions. It has first-class support for reasoning items.

use async_trait::async_trait;
use futures::StreamExt;
use reqwest_eventsource::EventSource;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::debug;

use super::model::ModelConfig;
use super::traits::*;
use crate::types::*;

pub struct OpenAiResponsesProvider;

#[async_trait]
impl StreamProvider for OpenAiResponsesProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<Message, ProviderError> {
        let model_config = config
            .model_config
            .as_ref()
            .ok_or_else(|| ProviderError::Other("ModelConfig required".into()))?;

        let url = format!("{}/responses", model_config.base_url);
        let body = build_request_body(&config, model_config);
        debug!(
            "OpenAI Responses request: model={} url={}",
            config.model, url
        );

        let client = reqwest::Client::new();
        let mut request = client
            .post(&url)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", config.api_key));

        for (k, v) in &model_config.headers {
            request = request.header(k, v);
        }

        let request = request.json(&body);
        let mut es =
            EventSource::new(request).map_err(|e| ProviderError::Network(e.to_string()))?;

        let mut content: Vec<Content> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = StopReason::Stop;
        let mut tool_call_buffers: std::collections::HashMap<usize, ToolCallBuffer> =
            std::collections::HashMap::new();

        let _ = tx.send(StreamEvent::Start);

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    es.close();
                    return Err(ProviderError::Cancelled);
                }
                event = es.next() => {
                    match event {
                        None => break,
                        Some(Ok(reqwest_eventsource::Event::Open)) => {}
                        Some(Ok(reqwest_eventsource::Event::Message(msg))) => {
                            match msg.event.as_str() {
                                "response.output_text.delta" => {
                                    if let Ok(data) = serde_json::from_str::<TextDeltaEvent>(&msg.data) {
                                        let text_idx = content.iter().position(|c| matches!(c, Content::Text { .. }));
                                        let idx = match text_idx {
                                            Some(i) => i,
                                            None => {
                                                content.push(Content::Text { text: String::new() });
                                                content.len() - 1
                                            }
                                        };
                                        if let Some(Content::Text { text }) = content.get_mut(idx) {
                                            text.push_str(&data.delta);
                                        }
                                        let _ = tx.send(StreamEvent::TextDelta {
                                            content_index: idx,
                                            delta: data.delta,
                                        });
                                    }
                                }
                                "response.reasoning.delta" => {
                                    if let Ok(data) = serde_json::from_str::<TextDeltaEvent>(&msg.data) {
                                        let idx = content.iter().position(|c| matches!(c, Content::Thinking { .. }));
                                        let idx = match idx {
                                            Some(i) => i,
                                            None => {
                                                content.push(Content::Thinking { thinking: String::new(), signature: None });
                                                content.len() - 1
                                            }
                                        };
                                        if let Some(Content::Thinking { thinking, .. }) = content.get_mut(idx) {
                                            thinking.push_str(&data.delta);
                                        }
                                        let _ = tx.send(StreamEvent::ThinkingDelta {
                                            content_index: idx,
                                            delta: data.delta,
                                        });
                                    }
                                }
                                "response.function_call_arguments.start" => {
                                    if let Ok(data) = serde_json::from_str::<FunctionCallStartEvent>(&msg.data) {
                                        let idx = content.len() + tool_call_buffers.len();
                                        tool_call_buffers.insert(idx, ToolCallBuffer {
                                            id: data.call_id.unwrap_or_default(),
                                            name: data.name.unwrap_or_default(),
                                            arguments: String::new(),
                                        });
                                        let buf = &tool_call_buffers[&idx];
                                        let _ = tx.send(StreamEvent::ToolCallStart {
                                            content_index: idx,
                                            id: buf.id.clone(),
                                            name: buf.name.clone(),
                                        });
                                    }
                                }
                                "response.function_call_arguments.delta" => {
                                    if let Ok(data) = serde_json::from_str::<TextDeltaEvent>(&msg.data) {
                                        // Find last buffer
                                        if let Some((&idx, buf)) = tool_call_buffers.iter_mut().last() {
                                            buf.arguments.push_str(&data.delta);
                                            let _ = tx.send(StreamEvent::ToolCallDelta {
                                                content_index: idx,
                                                delta: data.delta,
                                            });
                                        }
                                    }
                                }
                                "response.function_call_arguments.done" => {
                                    // Tool call complete
                                }
                                "response.completed" => {
                                    if let Ok(data) = serde_json::from_str::<ResponseCompletedEvent>(&msg.data) {
                                        if let Some(resp) = data.response {
                                            if let Some(u) = resp.usage {
                                                usage.input = u.input_tokens;
                                                usage.output = u.output_tokens;
                                                usage.total_tokens = u.total_tokens;
                                            }
                                            if resp.status == Some("incomplete".to_string()) {
                                                stop_reason = StopReason::Length;
                                            }
                                        }
                                    }
                                    break;
                                }
                                "error" => {
                                    let provider_err = classify_sse_error_event(&msg.data);
                                    return Err(provider_err);
                                }
                                _ => {
                                    debug!("Unknown Responses event: {}", msg.event);
                                }
                            }
                        }
                        Some(Err(e)) => {
                            let provider_err = classify_eventsource_error(e).await;
                            return Err(provider_err);
                        }
                    }
                }
            }
        }

        // Finalize tool calls
        for (_, buf) in tool_call_buffers {
            let args = super::json_repair::try_repair_json(&buf.arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            content.push(Content::ToolCall {
                id: buf.id,
                name: buf.name,
                arguments: args,
            });
        }

        if content
            .iter()
            .any(|c| matches!(c, Content::ToolCall { .. }))
        {
            stop_reason = StopReason::ToolUse;
        }

        let message = Message::Assistant {
            content,
            stop_reason,
            model: config.model.clone(),
            provider: model_config.provider.clone(),
            usage,
            timestamp: now_ms(),
            error_message: None,
        };

        let _ = tx.send(StreamEvent::Done {
            message: message.clone(),
        });
        Ok(message)
    }
}

struct ToolCallBuffer {
    id: String,
    name: String,
    arguments: String,
}

fn build_request_body(config: &StreamConfig, _model_config: &ModelConfig) -> serde_json::Value {
    let mut input: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                // Build content array for user message (supports text + images)
                let user_content: Vec<serde_json::Value> = content
                    .iter()
                    .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
                    .filter_map(|c| match c {
                        Content::Text { text } => Some(serde_json::json!({
                            "type": "input_text",
                            "text": text,
                        })),
                        Content::Image { data, mime_type } => Some(serde_json::json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", mime_type, data),
                        })),
                        _ => None,
                    })
                    .collect();

                if user_content.len() == 1 && user_content[0]["type"] == "input_text" {
                    // Simple text-only message can use shorthand format
                    input.push(serde_json::json!({
                        "role": "user",
                        "content": user_content[0]["text"].as_str().unwrap_or(""),
                    }));
                } else {
                    // Multi-modal content uses array format
                    input.push(serde_json::json!({
                        "role": "user",
                        "content": user_content,
                    }));
                }
            }
            Message::Assistant { content, .. } => {
                for c in content {
                    match c {
                        Content::Text { text } if text.is_empty() => {}
                        Content::Text { text } => {
                            input.push(serde_json::json!({
                                "type": "message",
                                "role": "assistant",
                                "content": [{"type": "output_text", "text": text}],
                            }));
                        }
                        Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            input.push(serde_json::json!({
                                "type": "function_call",
                                "call_id": id,
                                "name": name,
                                "arguments": arguments.to_string(),
                            }));
                        }
                        _ => {}
                    }
                }
            }
            Message::ToolResult {
                tool_call_id,
                content,
                ..
            } => {
                let output_val = if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                    // Images present: build content array
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
                        .filter_map(|c| match c {
                            Content::Text { text } => Some(serde_json::json!({
                                "type": "input_text",
                                "text": text,
                            })),
                            Content::Image { data, mime_type } => Some(serde_json::json!({
                                "type": "input_image",
                                "image_url": format!("data:{};base64,{}", mime_type, data),
                            })),
                            _ => None,
                        })
                        .collect();
                    serde_json::json!(parts)
                } else {
                    let text = content
                        .iter()
                        .find_map(|c| match c {
                            Content::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    serde_json::json!(text)
                };
                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id,
                    "output": output_val,
                }));
            }
        }
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "stream": true,
        "input": input,
    });

    if !config.system_prompt.is_empty() {
        body["instructions"] = serde_json::json!(config.system_prompt);
    }

    if let Some(max) = config.max_tokens {
        body["max_output_tokens"] = serde_json::json!(max);
    }

    if !config.tools.is_empty() {
        let tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body["tools"] = serde_json::json!(tools);
    }

    if config.thinking_level != ThinkingLevel::Off {
        let effort = match config.thinking_level {
            ThinkingLevel::Minimal | ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::Off => unreachable!(),
        };
        body["reasoning"] = serde_json::json!({"effort": effort});
    }

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

// Event types
#[derive(Deserialize)]
struct TextDeltaEvent {
    delta: String,
}

#[derive(Deserialize)]
struct FunctionCallStartEvent {
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ResponseCompletedEvent {
    #[serde(default)]
    response: Option<ResponseData>,
}

#[derive(Deserialize)]
struct ResponseData {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    usage: Option<ResponseUsage>,
}

#[derive(Deserialize)]
struct ResponseUsage {
    #[serde(default)]
    input_tokens: u64,
    #[serde(default)]
    output_tokens: u64,
    #[serde(default)]
    total_tokens: u64,
}
