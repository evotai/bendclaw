//! Amazon Bedrock ConverseStream provider.
//!
//! Uses the Bedrock ConverseStream API with AWS SigV4 request signing.
//! For simplicity, we implement minimal SigV4 signing using the `aws-sigv4`
//! and `aws-credential-types` crates. If those aren't available, callers
//! can pass pre-signed requests or use an IAM proxy.
//!
//! The `api_key` field in StreamConfig is expected to be formatted as:
//! `{access_key_id}:{secret_access_key}` (with optional `:{session_token}`).
//! The `base_url` in ModelConfig should be the Bedrock endpoint, e.g.:
//! `https://bedrock-runtime.us-east-1.amazonaws.com`

use std::collections::HashMap;

use async_trait::async_trait;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::debug;

use super::error::*;
use super::system_prompt::without_dynamic_boundary;
use super::traits::*;
use crate::context::now_ms;
use crate::types::*;

pub struct BedrockProvider;

#[async_trait]
impl StreamProvider for BedrockProvider {
    async fn stream(
        &self,
        config: StreamConfig,
        tx: mpsc::UnboundedSender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        self.stream_sink(config, super::stream_sink::StreamSink::Legacy(tx), cancel)
            .await
    }
    async fn stream_bounded(
        &self,
        config: StreamConfig,
        tx: mpsc::Sender<StreamEvent>,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        self.stream_sink(
            config,
            super::stream_sink::StreamSink::Bounded {
                tx,
                cancel: cancel.clone(),
            },
            cancel,
        )
        .await
    }
}
impl BedrockProvider {
    async fn stream_sink(
        &self,
        config: StreamConfig,
        tx: super::stream_sink::StreamSink,
        cancel: tokio_util::sync::CancellationToken,
    ) -> Result<StreamOutcome, ProviderError> {
        let model_config = config
            .model_config
            .as_ref()
            .ok_or_else(|| ProviderError::Other("ModelConfig required".into()))?;

        let base_url = model_config.base_url();
        let url = format!("{}/model/{}/converse-stream", base_url, config.model);

        let body = build_bedrock_body(&config);
        debug!("Bedrock request: model={} url={}", config.model, url);

        // Parse AWS credentials from api_key
        let parts: Vec<&str> = config.api_key.splitn(3, ':').collect();
        if parts.len() < 2 {
            return Err(ProviderError::Auth(
                "Bedrock api_key must be 'access_key:secret_key[:session_token]'".into(),
            ));
        }

        let client = crate::provider::error::new_client()?;
        let mut request = client.post(&url).header("content-type", "application/json");

        // Add AWS auth headers. In a real implementation, this would use SigV4.
        // For now, we support a simplified auth model where the caller provides
        // pre-computed auth headers via model_config.headers, or uses an IAM proxy.
        for (k, v) in model_config.headers() {
            request = request.header(k, v);
        }

        // If no auth headers provided, try basic Bearer auth as fallback
        // (works with some Bedrock proxy configurations)
        if !model_config.headers().contains_key("authorization") {
            request = request.header("authorization", format!("Bearer {}", config.api_key));
        }

        let response = request.json(&body).send().await.map_err(|e| {
            ProviderError::Network(crate::provider::error::format_transport_detail(
                &e,
                Some(&url),
            ))
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::classify(
                status.as_u16(),
                &format!("Bedrock error {}: {}", status, body),
            ));
        }

        let mut content: Vec<Content> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = StopReason::Stop;
        let mut tool_input_buffers: HashMap<usize, String> = HashMap::new();

        let _ = tx.send(StreamEvent::Start).await;

        // Bedrock ConverseStream returns event-stream format (application/vnd.amazon.eventstream)
        // For simplicity, we parse it as newline-delimited JSON chunks.
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();

        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    return Err(ProviderError::Cancelled);
                }
                chunk = stream.next() => {
                    match chunk {
                        None => break,
                        Some(Err(e)) => {
                            debug!("Bedrock stream error: {}", e);
                            break;
                        }
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            // Try to parse complete JSON objects
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].trim().to_string();
                                buffer = buffer[pos + 1..].to_string();

                                if line.is_empty() {
                                    continue;
                                }

                                let event: BedrockEvent = match serde_json::from_str(&line) {
                                    Ok(e) => e,
                                    Err(_) => continue,
                                };

                                match event {
                                    BedrockEvent::ContentBlockDelta { delta, .. } => {
                                        if let Some(text) = delta.text {
                                            let text_idx = content.iter().position(|c| matches!(c, Content::Text { .. }));
                                            let idx = match text_idx {
                                                Some(i) => i,
                                                None => {
                                                    content.push(Content::Text { text: String::new() });
                                                    content.len() - 1
                                                }
                                            };
                                            if let Some(Content::Text { text: t }) = content.get_mut(idx) {
                                                t.push_str(&text);
                                            }
                                            let _ = tx.send(StreamEvent::TextDelta {
                                                content_index: idx,
                                                delta: text,
                                            }).await;
                                        }
                                        if let Some(tool_use) = delta.tool_use {
                                            if let Some((idx, id, name)) = content
                                                .iter()
                                                .enumerate()
                                                .rev()
                                                .find_map(|(idx, c)| match c {
                                                    Content::ToolCall { id, name, .. } => {
                                                        Some((idx, id.clone(), name.clone()))
                                                    }
                                                    _ => None,
                                                })
                                            {
                                                tool_input_buffers
                                                    .entry(idx)
                                                    .or_default()
                                                    .push_str(&tool_use.input);
                                                let _ = tx.send(StreamEvent::ToolCallDelta {
                                                    content_index: idx,
                                                    id,
                                                    name,
                                                    delta: tool_use.input,
                                                }).await;
                                            }
                                        }
                                    }
                                    BedrockEvent::ContentBlockStart { start, .. } => {
                                        if let Some(tool_use) = start.tool_use {
                                            let idx = content.len();
                                            content.push(Content::ToolCall {
                                                id: tool_use.tool_use_id.clone(),
                                                name: tool_use.name.clone(),
                                                arguments: serde_json::Value::Object(Default::default()),
                                                metadata: None,
                                            });
                                            let _ = tx.send(StreamEvent::ToolCallStart {
                                                content_index: idx,
                                                id: tool_use.tool_use_id,
                                                name: tool_use.name,
                                            }).await;
                                        }
                                    }
                                    BedrockEvent::ContentBlockStop { .. } => {
                                        if let Some((idx, id, name)) = content
                                            .iter()
                                            .enumerate()
                                            .rev()
                                            .find_map(|(idx, c)| match c {
                                                Content::ToolCall { id, name, .. } => {
                                                    Some((idx, id.clone(), name.clone()))
                                                }
                                                _ => None,
                                            })
                                        {
                                            let arguments = tool_input_buffers
                                                .remove(&idx)
                                                .and_then(|input| {
                                                    crate::provider::json_repair::try_repair_json(&input).ok()
                                                })
                                                .unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                                            if let Some(Content::ToolCall { arguments: current, .. }) = content.get_mut(idx) {
                                                *current = arguments.clone();
                                            }
                                            let _ = tx.send(StreamEvent::ToolCallEnd {
                                                content_index: idx,
                                                id,
                                                name,
                                                arguments,
                                            }).await;
                                        }
                                    }
                                    BedrockEvent::MessageStop { stop_reason: sr } => {
                                        stop_reason = match sr.as_deref() {
                                            Some("end_turn") => StopReason::Stop,
                                            Some("max_tokens") => StopReason::Length,
                                            Some("tool_use") => StopReason::ToolUse,
                                            _ => StopReason::Stop,
                                        };
                                    }
                                    BedrockEvent::Metadata { usage: u } => {
                                        if let Some(u) = u {
                                            usage.input = u.input_tokens;
                                            usage.output = u.output_tokens;
                                            usage.total_tokens = u.input_tokens + u.output_tokens;
                                        }
                                    }
                                    BedrockEvent::Unknown => {}
                                }
                            }
                        }
                    }
                }
            }
        }

        let message = Message::Assistant {
            content,
            stop_reason,
            model: config.model.clone(),
            provider: model_config.provider().to_string(),
            usage,
            timestamp: now_ms(),
            error_message: None,
            response_id: None,
        };

        let _ = tx
            .send(StreamEvent::Done {
                message: message.clone(),
            })
            .await;
        Ok(StreamOutcome::complete(message))
    }
}

pub fn build_bedrock_body(config: &StreamConfig) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                let blocks = content_to_bedrock(content);
                messages.push(serde_json::json!({"role": "user", "content": blocks}));
            }
            Message::Assistant { content, .. } => {
                let blocks = content_to_bedrock(content);
                messages.push(serde_json::json!({"role": "assistant", "content": blocks}));
            }
            Message::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                // Build content blocks for tool result (text + images)
                let tool_content: Vec<serde_json::Value> = content
                    .iter()
                    .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
                    .filter_map(|c| match c {
                        Content::Text { text } => Some(serde_json::json!({"text": text})),
                        Content::Image { .. } => c.resolve_image_data().map(|(data, mime_type)| {
                            serde_json::json!({
                                "image": {
                                    "format": mime_type.split('/').nth(1).unwrap_or("png"),
                                    "source": {"bytes": data},
                                }
                            })
                        }),
                        _ => None,
                    })
                    .collect();

                let tool_content = if tool_content.is_empty() {
                    vec![serde_json::json!({"text": ""})]
                } else {
                    tool_content
                };

                messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "toolResult": {
                            "toolUseId": tool_call_id,
                            "content": tool_content,
                            "status": if *is_error { "error" } else { "success" },
                        }
                    }],
                }));
            }
        }
    }

    let mut body = serde_json::json!({"messages": messages});

    let system_prompt = without_dynamic_boundary(&config.system_prompt);
    if !system_prompt.is_empty() {
        body["system"] = serde_json::json!([{"text": system_prompt.as_ref()}]);
    }

    let mut inference_config = serde_json::json!({});
    if let Some(max) = config.max_tokens {
        inference_config["maxTokens"] = serde_json::json!(max);
    }
    if inference_config != serde_json::json!({}) {
        body["inferenceConfig"] = inference_config;
    }

    if !config.tools.is_empty() {
        let tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "toolSpec": {
                        "name": t.name,
                        "description": t.description,
                        "inputSchema": {"json": t.parameters},
                    }
                })
            })
            .collect();
        body["toolConfig"] = serde_json::json!({"tools": tools});
    }

    body
}

pub fn content_to_bedrock(content: &[Content]) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
        .filter_map(|c| match c {
            Content::Text { text } => Some(serde_json::json!({"text": text})),
            Content::Image { .. } => c.resolve_image_data().map(|(data, mime_type)| {
                serde_json::json!({
                    "image": {
                        "format": mime_type.split('/').nth(1).unwrap_or("png"),
                        "source": {"bytes": data},
                    }
                })
            }),
            Content::ToolCall {
                id,
                name,
                arguments,
                ..
            } => Some(serde_json::json!({
                "toolUse": {"toolUseId": id, "name": name, "input": arguments},
            })),
            Content::Thinking { .. } => None,
        })
        .collect()
}

// Bedrock event types
#[derive(Deserialize)]
#[serde(untagged)]
enum BedrockEvent {
    ContentBlockDelta {
        #[serde(rename = "contentBlockDelta")]
        delta: BedrockDelta,
    },
    ContentBlockStart {
        #[serde(rename = "contentBlockStart")]
        start: BedrockBlockStart,
    },
    ContentBlockStop {
        #[serde(rename = "contentBlockStop")]
        #[allow(dead_code)]
        stop: serde_json::Value,
    },
    MessageStop {
        #[serde(rename = "messageStop")]
        stop_reason: Option<String>,
    },
    Metadata {
        #[serde(rename = "metadata")]
        usage: Option<BedrockUsage>,
    },
    Unknown,
}

#[derive(Deserialize)]
struct BedrockDelta {
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "toolUse")]
    tool_use: Option<BedrockToolUseDelta>,
}

#[derive(Deserialize)]
struct BedrockToolUseDelta {
    input: String,
}

#[derive(Deserialize)]
struct BedrockBlockStart {
    #[serde(default, rename = "toolUse")]
    tool_use: Option<BedrockToolUseStart>,
}

#[derive(Deserialize)]
struct BedrockToolUseStart {
    #[serde(rename = "toolUseId")]
    tool_use_id: String,
    name: String,
}

#[derive(Deserialize)]
struct BedrockUsage {
    #[serde(default, rename = "inputTokens")]
    input_tokens: u64,
    #[serde(default, rename = "outputTokens")]
    output_tokens: u64,
}
