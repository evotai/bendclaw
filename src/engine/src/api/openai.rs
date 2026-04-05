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
use crate::types::ContentBlock;
use crate::types::Message;
use crate::types::MessageRole;
use crate::types::Usage;

const DEFAULT_BASE_URL: &str = "https://api.openai.com";

// --- OpenAI request types ---

#[derive(Debug, Serialize)]
struct OpenAIRequest {
    model: String,
    max_tokens: u64,
    messages: Vec<OpenAIMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OpenAITool>>,
    stream: bool,
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

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
struct OpenAIFunctionCall {
    name: String,
    arguments: String,
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
        // Convert messages to OpenAI format
        let mut openai_messages = Vec::new();

        // System prompt as first message
        if let Some(system_blocks) = &request.system {
            let system_text: String = system_blocks
                .iter()
                .map(|b| b.text.as_str())
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

        // Convert conversation messages
        for msg in request.messages {
            match msg.role {
                MessageRole::User => {
                    // Check if this is a tool_result message
                    let tool_results: Vec<&ContentBlock> = msg
                        .content
                        .iter()
                        .filter(|b| matches!(b, ContentBlock::ToolResult { .. }))
                        .collect();

                    if !tool_results.is_empty() {
                        // Each tool_result becomes a separate "tool" role message
                        for block in &tool_results {
                            if let ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                ..
                            } = block
                            {
                                let text: String = content
                                    .iter()
                                    .filter_map(|c| match c {
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
                        // Regular user message
                        let text = crate::types::extract_text(msg);
                        openai_messages.push(OpenAIMessage {
                            role: "user".to_string(),
                            content: Some(Value::String(text)),
                            tool_calls: None,
                            tool_call_id: None,
                        });
                    }
                }
                MessageRole::Assistant => {
                    let text = crate::types::extract_text(msg);
                    let tool_uses: Vec<OpenAIToolCall> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
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
                        tool_calls: if tool_uses.is_empty() {
                            None
                        } else {
                            Some(tool_uses)
                        },
                        tool_call_id: None,
                    });
                }
            }
        }

        // Convert tools to OpenAI format
        let openai_tools: Option<Vec<OpenAITool>> = request.tools.map(|tools| {
            tools
                .into_iter()
                .map(|t| OpenAITool {
                    tool_type: "function".to_string(),
                    function: OpenAIFunction {
                        name: t.name,
                        description: t.description,
                        parameters: t.input_schema,
                    },
                })
                .collect()
        });

        let body = OpenAIRequest {
            model: request.model.to_string(),
            max_tokens: request.max_tokens,
            messages: openai_messages,
            tools: openai_tools.filter(|t| !t.is_empty()),
            stream: true,
        };

        let mut req_builder = self
            .client
            .post(self.chat_completions_url())
            .header("authorization", format!("Bearer {}", self.api_key))
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

        parse_openai_stream(response).await
    }
}

/// Parse OpenAI SSE stream into a ProviderResponse.
async fn parse_openai_stream(response: reqwest::Response) -> Result<ProviderResponse, ApiError> {
    let body = response
        .text()
        .await
        .map_err(|e| ApiError::NetworkError(e.to_string()))?;

    let mut text_content = String::new();
    let mut tool_calls: HashMap<usize, OpenAIToolCall> = HashMap::new();
    let mut usage = Usage::default();
    let mut stop_reason: Option<String> = None;

    for line in body.lines() {
        let line = line.trim();
        if !line.starts_with("data: ") {
            continue;
        }
        let data = &line[6..];
        if data == "[DONE]" {
            break;
        }

        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(error) = response::stream_error(&chunk) {
            return Err(error);
        }

        // Parse usage if present
        if let Some(u) = chunk.get("usage") {
            if let Some(pt) = u.get("prompt_tokens").and_then(|v| v.as_u64()) {
                usage.input_tokens = pt;
            }
            if let Some(ct) = u.get("completion_tokens").and_then(|v| v.as_u64()) {
                usage.output_tokens = ct;
            }
        }

        // Parse choices
        if let Some(choices) = chunk.get("choices").and_then(|c| c.as_array()) {
            for choice in choices {
                // Check finish_reason
                if let Some(fr) = choice.get("finish_reason").and_then(|f| f.as_str()) {
                    stop_reason = Some(match fr {
                        "stop" => "end_turn".to_string(),
                        "tool_calls" => "tool_use".to_string(),
                        "length" => "max_tokens".to_string(),
                        other => other.to_string(),
                    });
                }

                let delta = match choice.get("delta") {
                    Some(d) => d,
                    None => continue,
                };

                // Text content
                if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                    text_content.push_str(content);
                }

                // Tool calls
                if let Some(tcs) = delta.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tcs {
                        let idx = tc.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;

                        let entry = tool_calls.entry(idx).or_insert_with(|| OpenAIToolCall {
                            id: String::new(),
                            call_type: "function".to_string(),
                            function: OpenAIFunctionCall {
                                name: String::new(),
                                arguments: String::new(),
                            },
                        });

                        if let Some(id) = tc.get("id").and_then(|i| i.as_str()) {
                            entry.id = id.to_string();
                        }
                        if let Some(func) = tc.get("function") {
                            if let Some(name) = func.get("name").and_then(|n| n.as_str()) {
                                entry.function.name = name.to_string();
                            }
                            if let Some(args) = func.get("arguments").and_then(|a| a.as_str()) {
                                entry.function.arguments.push_str(args);
                            }
                        }
                    }
                }
            }
        }
    }

    // Build normalized content blocks
    let mut content_blocks: Vec<ContentBlock> = Vec::new();

    if !text_content.is_empty() {
        content_blocks.push(ContentBlock::Text { text: text_content });
    }

    // Convert tool calls to ContentBlock::ToolUse
    let mut sorted_calls: Vec<(usize, OpenAIToolCall)> = tool_calls.into_iter().collect();
    sorted_calls.sort_by_key(|(idx, _)| *idx);
    for (_, tc) in sorted_calls {
        let input: Value = serde_json::from_str(&tc.function.arguments)
            .unwrap_or(Value::Object(serde_json::Map::new()));
        content_blocks.push(ContentBlock::ToolUse {
            id: if tc.id.is_empty() {
                format!("call_{}", uuid::Uuid::new_v4())
            } else {
                tc.id
            },
            name: tc.function.name,
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
