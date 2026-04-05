use std::collections::HashMap;

use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use super::provider::ApiType;
use super::provider::LLMProvider;
use super::provider::ProviderRequest;
use super::provider::ProviderResponse;
use super::response;
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
        let api_messages: Vec<AnthropicMessage> = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                };
                AnthropicMessage {
                    role: role.to_string(),
                    content: serde_json::to_value(&m.content).unwrap_or(Value::Array(vec![])),
                }
            })
            .collect();

        let body = AnthropicRequest {
            model: request.model.to_string(),
            max_tokens: request.max_tokens,
            messages: api_messages,
            system: request.system,
            tools: if request.tools.as_ref().is_none_or(|t| t.is_empty()) {
                None
            } else {
                request.tools
            },
            stream: true,
            thinking: request.thinking,
        };

        let mut req_builder = self
            .client
            .post(self.messages_url())
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", API_VERSION)
            .header("anthropic-beta", "prompt-caching-2024-07-31")
            .header("content-type", "application/json");

        for (key, value) in &self.custom_headers {
            req_builder = req_builder.header(key, value);
        }

        let response = req_builder.json(&body).send().await.map_err(|e| {
            if e.is_timeout() {
                ApiError::Timeout
            } else {
                ApiError::NetworkError(e.to_string())
            }
        })?;

        if !response.status().is_success() {
            return Err(response::http_error(response).await);
        }

        parse_anthropic_stream(response).await
    }
}

/// Parse Anthropic SSE stream into a ProviderResponse.
async fn parse_anthropic_stream(response: reqwest::Response) -> Result<ProviderResponse, ApiError> {
    let body = response
        .text()
        .await
        .map_err(|e| ApiError::NetworkError(e.to_string()))?;

    let mut content_blocks: Vec<ContentBlock> = Vec::new();
    let mut usage = Usage::default();
    let mut stop_reason: Option<String> = None;
    let mut current_blocks: HashMap<usize, Value> = HashMap::new();

    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let event: Value = match serde_json::from_str(data) {
            Ok(e) => e,
            Err(_) => continue,
        };

        if let Some(error) = response::stream_error(&event) {
            return Err(error);
        }

        let event_type = event.get("type").and_then(|t| t.as_str()).unwrap_or("");

        match event_type {
            "message_start" => {
                if let Some(u) = event.pointer("/message/usage") {
                    if let Ok(u) = serde_json::from_value::<Usage>(u.clone()) {
                        usage.input_tokens = u.input_tokens;
                        usage.cache_creation_input_tokens = u.cache_creation_input_tokens;
                        usage.cache_read_input_tokens = u.cache_read_input_tokens;
                    }
                }
            }
            "content_block_start" => {
                if let (Some(idx), Some(block)) = (
                    event
                        .get("index")
                        .and_then(|i| i.as_u64())
                        .map(|i| i as usize),
                    event.get("content_block"),
                ) {
                    current_blocks.insert(idx, block.clone());
                }
            }
            "content_block_delta" => {
                let idx = event
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .map(|i| i as usize);
                let delta = event.get("delta");
                if let (Some(idx), Some(delta)) = (idx, delta) {
                    let delta_type = delta.get("type").and_then(|t| t.as_str()).unwrap_or("");
                    match delta_type {
                        "text_delta" => {
                            if let Some(text) = delta.get("text").and_then(|t| t.as_str()) {
                                let block = current_blocks.entry(idx).or_insert_with(
                                    || serde_json::json!({"type": "text", "text": ""}),
                                );
                                let existing =
                                    block.get("text").and_then(|t| t.as_str()).unwrap_or("");
                                block["text"] = Value::String(format!("{}{}", existing, text));
                            }
                        }
                        "input_json_delta" => {
                            if let Some(partial) =
                                delta.get("partial_json").and_then(|t| t.as_str())
                            {
                                let block = current_blocks.entry(idx).or_insert_with(|| {
                                    serde_json::json!({"type": "tool_use", "id": "", "name": "", "input": {}})
                                });
                                let existing = block
                                    .get("_partial_json")
                                    .and_then(|t| t.as_str())
                                    .unwrap_or("");
                                block["_partial_json"] =
                                    Value::String(format!("{}{}", existing, partial));
                            }
                        }
                        "thinking_delta" => {
                            if let Some(thinking) = delta.get("thinking").and_then(|t| t.as_str()) {
                                let block = current_blocks.entry(idx).or_insert_with(
                                    || serde_json::json!({"type": "thinking", "thinking": ""}),
                                );
                                let existing =
                                    block.get("thinking").and_then(|t| t.as_str()).unwrap_or("");
                                block["thinking"] =
                                    Value::String(format!("{}{}", existing, thinking));
                            }
                        }
                        "signature_delta" => {
                            if let Some(sig) = delta.get("signature").and_then(|t| t.as_str()) {
                                if let Some(block) = current_blocks.get_mut(&idx) {
                                    block["signature"] = Value::String(sig.to_string());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            "content_block_stop" => {
                let idx = event
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .map(|i| i as usize);
                if let Some(idx) = idx {
                    if let Some(block) = current_blocks.remove(&idx) {
                        if let Some(cb) = parse_anthropic_content_block(block) {
                            content_blocks.push(cb);
                        }
                    }
                }
            }
            "message_delta" => {
                if let Some(sr) = event.pointer("/delta/stop_reason").and_then(|s| s.as_str()) {
                    stop_reason = Some(sr.to_string());
                }
                if let Some(u) = event.get("usage") {
                    if let Some(out) = u.get("output_tokens").and_then(|o| o.as_u64()) {
                        usage.output_tokens = out;
                    }
                }
            }
            _ => {}
        }
    }

    // Flush remaining blocks
    let mut remaining: Vec<(usize, Value)> = current_blocks.into_iter().collect();
    remaining.sort_by_key(|(idx, _)| *idx);
    for (_, block) in remaining {
        if let Some(cb) = parse_anthropic_content_block(block) {
            content_blocks.push(cb);
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

fn parse_anthropic_content_block(mut block: Value) -> Option<ContentBlock> {
    let block_type = block.get("type")?.as_str()?.to_string();
    match block_type.as_str() {
        "text" => Some(ContentBlock::Text {
            text: block.get("text")?.as_str()?.to_string(),
        }),
        "tool_use" => {
            let id = block.get("id")?.as_str()?.to_string();
            let name = block.get("name")?.as_str()?.to_string();
            let input = if let Some(partial) = block.get("_partial_json").and_then(|p| p.as_str()) {
                serde_json::from_str(partial).unwrap_or(Value::Object(serde_json::Map::new()))
            } else {
                block
                    .get("input")
                    .cloned()
                    .unwrap_or(Value::Object(serde_json::Map::new()))
            };
            if let Some(obj) = block.as_object_mut() {
                obj.remove("_partial_json");
            }
            Some(ContentBlock::ToolUse { id, name, input })
        }
        "thinking" => Some(ContentBlock::Thinking {
            thinking: block.get("thinking")?.as_str()?.to_string(),
            signature: block
                .get("signature")
                .and_then(|s| s.as_str())
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
