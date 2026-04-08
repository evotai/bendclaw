//! Google Generative AI (Gemini) provider.
//!
//! Uses the `streamGenerateContent` endpoint with SSE streaming.
//! API key is passed as a query parameter.

use async_trait::async_trait;
use futures::StreamExt;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::warn;

use super::traits::*;
use crate::types::*;

pub struct GoogleProvider;

#[async_trait]
impl StreamProvider for GoogleProvider {
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

        let base_url = &model_config.base_url;
        let url = format!(
            "{}/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            base_url, config.model, config.api_key
        );

        let body = build_request_body(&config);
        debug!("Google GenAI request: model={}", config.model);

        let client = reqwest::Client::new();
        let mut request = client.post(&url).header("content-type", "application/json");

        for (k, v) in &model_config.headers {
            request = request.header(k, v);
        }

        // Google streams JSON chunks separated by newlines, not SSE.
        // With alt=sse, it does use SSE format.
        let response = request
            .json(&body)
            .send()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProviderError::classify(
                status.as_u16(),
                &format!("Google API error {}: {}", status, body),
            ));
        }

        let mut content: Vec<Content> = Vec::new();
        let mut usage = Usage::default();
        let mut stop_reason = StopReason::Stop;

        let _ = tx.send(StreamEvent::Start);

        // Parse SSE stream
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
                            warn!("Google stream error: {}", e);
                            break;
                        }
                        Some(Ok(bytes)) => {
                            buffer.push_str(&String::from_utf8_lossy(&bytes));

                            // Process complete SSE events
                            while let Some(pos) = buffer.find("\n\n") {
                                let event_str = buffer[..pos].to_string();
                                buffer = buffer[pos + 2..].to_string();

                                // Parse SSE data line
                                let data = event_str
                                    .lines()
                                    .find(|l| l.starts_with("data: "))
                                    .map(|l| &l[6..])
                                    .unwrap_or("");

                                if data.is_empty() {
                                    continue;
                                }

                                let chunk: GoogleChunk = match serde_json::from_str(data) {
                                    Ok(c) => c,
                                    Err(e) => {
                                        debug!("Failed to parse Google chunk: {}", e);
                                        continue;
                                    }
                                };

                                // Process candidates
                                for candidate in &chunk.candidates.unwrap_or_default() {
                                    if let Some(c) = &candidate.content {
                                        for part in &c.parts {
                                            if let Some(text) = &part.text {
                                                let text_idx = content.iter().position(|c| matches!(c, Content::Text { .. }));
                                                let idx = match text_idx {
                                                    Some(i) => i,
                                                    None => {
                                                        content.push(Content::Text { text: String::new() });
                                                        content.len() - 1
                                                    }
                                                };
                                                if let Some(Content::Text { text: t }) = content.get_mut(idx) {
                                                    t.push_str(text);
                                                }
                                                let _ = tx.send(StreamEvent::TextDelta {
                                                    content_index: idx,
                                                    delta: text.clone(),
                                                });
                                            }
                                            if let Some(fc) = &part.function_call {
                                                let id = format!("google-fc-{}", content.len());
                                                let args = fc.args.clone().unwrap_or(serde_json::Value::Object(Default::default()));
                                                let idx = content.len();
                                                content.push(Content::ToolCall {
                                                    id: id.clone(),
                                                    name: fc.name.clone(),
                                                    arguments: args,
                                                });
                                                let _ = tx.send(StreamEvent::ToolCallStart {
                                                    content_index: idx,
                                                    id,
                                                    name: fc.name.clone(),
                                                });
                                                let _ = tx.send(StreamEvent::ToolCallEnd { content_index: idx });
                                                stop_reason = StopReason::ToolUse;
                                            }
                                        }
                                    }
                                    if let Some(reason) = &candidate.finish_reason {
                                        stop_reason = match reason.as_str() {
                                            "STOP" => StopReason::Stop,
                                            "MAX_TOKENS" | "RECITATION" => StopReason::Length,
                                            _ => StopReason::Stop,
                                        };
                                    }
                                }

                                // Process usage
                                if let Some(u) = &chunk.usage_metadata {
                                    usage.input = u.prompt_token_count.unwrap_or(0);
                                    usage.output = u.candidates_token_count.unwrap_or(0);
                                    usage.total_tokens = u.total_token_count.unwrap_or(0);
                                    usage.cache_read = u.cached_content_token_count.unwrap_or(0);
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

fn build_request_body(config: &StreamConfig) -> serde_json::Value {
    let mut contents: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                let parts = content_to_google_parts(content);
                contents.push(serde_json::json!({
                    "role": "user",
                    "parts": parts,
                }));
            }
            Message::Assistant { content, .. } => {
                let parts = content_to_google_parts(content);
                contents.push(serde_json::json!({
                    "role": "model",
                    "parts": parts,
                }));
            }
            Message::ToolResult {
                tool_call_id: _,
                tool_name,
                content,
                ..
            } => {
                let text = content
                    .iter()
                    .find_map(|c| match c {
                        Content::Text { text } => Some(text.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();

                let mut parts = vec![serde_json::json!({
                    "functionResponse": {
                        "name": tool_name,
                        "response": {"result": text},
                    }
                })];

                // Append image parts if present
                for c in content {
                    if let Content::Image { data, mime_type } = c {
                        parts.push(serde_json::json!({
                            "inlineData": {"mimeType": mime_type, "data": data},
                        }));
                    }
                }

                contents.push(serde_json::json!({
                    "role": "user",
                    "parts": parts,
                }));
            }
        }
    }

    let mut body = serde_json::json!({
        "contents": contents,
    });

    if !config.system_prompt.is_empty() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{"text": config.system_prompt}],
        });
    }

    let mut generation_config = serde_json::json!({});
    if let Some(max) = config.max_tokens {
        generation_config["maxOutputTokens"] = serde_json::json!(max);
    }
    if let Some(temp) = config.temperature {
        generation_config["temperature"] = serde_json::json!(temp);
    }
    if generation_config != serde_json::json!({}) {
        body["generationConfig"] = generation_config;
    }

    if !config.tools.is_empty() {
        let declarations: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "parameters": t.parameters,
                })
            })
            .collect();
        body["tools"] = serde_json::json!([{
            "functionDeclarations": declarations,
        }]);
    }

    body
}

fn content_to_google_parts(content: &[Content]) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
        .filter_map(|c| match c {
            Content::Text { text } => Some(serde_json::json!({"text": text})),
            Content::Image { data, mime_type } => Some(serde_json::json!({
                "inlineData": {"mimeType": mime_type, "data": data},
            })),
            Content::ToolCall {
                name, arguments, ..
            } => Some(serde_json::json!({
                "functionCall": {"name": name, "args": arguments},
            })),
            Content::Thinking { .. } => None,
        })
        .collect()
}

// Google API response types
#[derive(Deserialize)]
struct GoogleChunk {
    #[serde(default)]
    candidates: Option<Vec<GoogleCandidate>>,
    #[serde(default, rename = "usageMetadata")]
    usage_metadata: Option<GoogleUsageMetadata>,
}

#[derive(Deserialize)]
struct GoogleCandidate {
    #[serde(default)]
    content: Option<GoogleContent>,
    #[serde(default, rename = "finishReason")]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct GoogleContent {
    #[serde(default)]
    parts: Vec<GooglePart>,
}

#[derive(Deserialize)]
struct GooglePart {
    #[serde(default)]
    text: Option<String>,
    #[serde(default, rename = "functionCall")]
    function_call: Option<GoogleFunctionCall>,
}

#[derive(Deserialize)]
struct GoogleFunctionCall {
    name: String,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct GoogleUsageMetadata {
    #[serde(default, rename = "promptTokenCount")]
    prompt_token_count: Option<u64>,
    #[serde(default, rename = "candidatesTokenCount")]
    candidates_token_count: Option<u64>,
    #[serde(default, rename = "totalTokenCount")]
    total_token_count: Option<u64>,
    #[serde(default, rename = "cachedContentTokenCount")]
    cached_content_token_count: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_google_request() {
        let config = StreamConfig {
            model: "gemini-2.0-flash".into(),
            system_prompt: "Be helpful".into(),
            messages: vec![Message::user("Hello")],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test".into(),
            max_tokens: Some(1024),
            temperature: Some(0.7),
            model_config: None,
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config);
        assert!(body["contents"].is_array());
        assert_eq!(body["contents"][0]["role"], "user");
        assert!(body["systemInstruction"].is_object());
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 1024);
        let temp = body["generationConfig"]["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.01);
    }

    #[test]
    fn test_content_to_google_parts_text() {
        let content = vec![Content::Text {
            text: "hello".into(),
        }];
        let parts = content_to_google_parts(&content);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "hello");
    }

    #[test]
    fn test_content_to_google_parts_filters_empty_text() {
        let content = vec![
            Content::Text { text: "".into() },
            Content::Text {
                text: "hello".into(),
            },
            Content::Text { text: "".into() },
        ];
        let parts = content_to_google_parts(&content);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "hello");
    }

    #[test]
    fn test_content_to_google_parts_tool_call() {
        let content = vec![Content::ToolCall {
            id: "tc-1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "ls"}),
        }];
        let parts = content_to_google_parts(&content);
        assert_eq!(parts[0]["functionCall"]["name"], "bash");
    }
}
