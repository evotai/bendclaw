//! OpenAI-compatible request body building and content conversion.

use crate::provider::model::MaxTokensField;
use crate::provider::model::ModelConfig;
use crate::provider::model::OpenAiCompat;
use crate::provider::traits::StreamConfig;
use crate::types::*;

#[derive(Default)]
pub(crate) struct ToolCallBuffer {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub(crate) fn build_request_body(
    config: &StreamConfig,
    model_config: &ModelConfig,
    compat: &OpenAiCompat,
) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // System prompt
    if !config.system_prompt.is_empty() {
        let role = if compat.supports_developer_role {
            "developer"
        } else {
            "system"
        };
        messages.push(serde_json::json!({
            "role": role,
            "content": config.system_prompt,
        }));
    }

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": content_to_openai(content),
                }));
            }
            Message::Assistant { content, .. } => {
                let mut parts: Vec<serde_json::Value> = Vec::new();
                let mut tool_calls: Vec<serde_json::Value> = Vec::new();

                for c in content {
                    match c {
                        Content::Text { text } if text.is_empty() => {}
                        Content::Text { text } => {
                            parts.push(serde_json::json!({"type": "text", "text": text}));
                        }
                        Content::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            tool_calls.push(serde_json::json!({
                                "id": id,
                                "type": "function",
                                "function": {"name": name, "arguments": arguments.to_string()},
                            }));
                        }
                        _ => {}
                    }
                }

                let mut msg_obj = serde_json::json!({"role": "assistant"});
                if !parts.is_empty() {
                    msg_obj["content"] = serde_json::json!(parts);
                }
                if !tool_calls.is_empty() {
                    msg_obj["tool_calls"] = serde_json::json!(tool_calls);
                }
                messages.push(msg_obj);
            }
            Message::ToolResult {
                tool_call_id,
                tool_name,
                content,
                ..
            } => {
                let content_val = if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                    content_to_openai(content)
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

                let mut msg_obj = serde_json::json!({
                    "role": "tool",
                    "tool_call_id": tool_call_id,
                    "content": content_val,
                });
                if compat.requires_tool_result_name {
                    msg_obj["name"] = serde_json::json!(tool_name);
                }
                messages.push(msg_obj);
            }
        }
    }

    let max_tokens_val = config.max_tokens.unwrap_or(model_config.max_tokens);
    let mut body = serde_json::json!({
        "model": config.model,
        "stream": true,
        "stream_options": {"include_usage": true},
        "messages": messages,
    });

    match compat.max_tokens_field {
        MaxTokensField::MaxCompletionTokens => {
            body["max_completion_tokens"] = serde_json::json!(max_tokens_val);
        }
        MaxTokensField::MaxTokens => {
            body["max_tokens"] = serde_json::json!(max_tokens_val);
        }
    }

    if !config.tools.is_empty() {
        let tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name,
                        "description": t.description,
                        "parameters": t.parameters,
                    }
                })
            })
            .collect();
        body["tools"] = serde_json::json!(tools);
    }

    if config.thinking_level != ThinkingLevel::Off && compat.supports_reasoning_effort {
        let effort = match config.thinking_level {
            ThinkingLevel::Minimal | ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::Off => unreachable!(),
        };
        body["reasoning_effort"] = serde_json::json!(effort);
    }

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

pub(crate) fn content_to_openai(content: &[Content]) -> serde_json::Value {
    if content.len() == 1 {
        if let Content::Text { text } = &content[0] {
            if !text.is_empty() {
                return serde_json::json!(text);
            }
        }
    }
    let parts: Vec<serde_json::Value> = content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
        .filter_map(|c| match c {
            Content::Text { text } => Some(serde_json::json!({"type": "text", "text": text})),
            Content::Image { data, mime_type } => Some(serde_json::json!({
                "type": "image_url",
                "image_url": {"url": format!("data:{};base64,{}", mime_type, data)},
            })),
            _ => None,
        })
        .collect();
    serde_json::json!(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::model::ModelConfig;
    use crate::provider::traits::ToolDefinition;

    #[test]
    fn test_build_request_body_basic() {
        let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
        let config = StreamConfig {
            model: "gpt-4o".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![Message::user("Hello")],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test".into(),
            max_tokens: None,
            temperature: None,
            model_config: Some(model_config.clone()),
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
        assert_eq!(body["model"], "gpt-4o");
        assert!(body["stream"].as_bool().unwrap());
        // Developer role for OpenAI
        assert_eq!(body["messages"][0]["role"], "developer");
        assert_eq!(body["messages"][1]["role"], "user");
        // max_completion_tokens for OpenAI
        assert!(body["max_completion_tokens"].is_number());
    }

    #[test]
    fn test_build_request_body_with_tools() {
        let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
        let compat = OpenAiCompat::openai();
        let config = StreamConfig {
            model: "gpt-4o".into(),
            system_prompt: String::new(),
            messages: vec![Message::user("List files")],
            tools: vec![ToolDefinition {
                name: "bash".into(),
                description: "Run a command".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            thinking_level: ThinkingLevel::Off,
            api_key: "test".into(),
            max_tokens: Some(1024),
            temperature: Some(0.5),
            model_config: Some(model_config.clone()),
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, &model_config, &compat);
        assert!(body["tools"].is_array());
        assert_eq!(body["tools"][0]["function"]["name"], "bash");
        assert_eq!(body["temperature"], 0.5);
    }

    #[test]
    fn test_content_to_openai_simple_text() {
        let content = vec![Content::Text {
            text: "hello".into(),
        }];
        let result = content_to_openai(&content);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_content_to_openai_filters_empty_text() {
        let content = vec![
            Content::Text { text: "".into() },
            Content::Text {
                text: "hello".into(),
            },
            Content::Text { text: "".into() },
        ];
        let result = content_to_openai(&content);
        let parts = result.as_array().unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0]["text"], "hello");
    }

    #[test]
    fn test_content_to_openai_single_empty_text_filtered() {
        let content = vec![Content::Text { text: "".into() }];
        let result = content_to_openai(&content);
        let parts = result.as_array().unwrap();
        assert!(parts.is_empty());
    }

    #[test]
    fn test_content_to_openai_multipart() {
        let content = vec![
            Content::Text {
                text: "look at this".into(),
            },
            Content::Image {
                data: "abc".into(),
                mime_type: "image/png".into(),
            },
        ];
        let result = content_to_openai(&content);
        assert!(result.is_array());
        assert_eq!(result[0]["type"], "text");
        assert_eq!(result[1]["type"], "image_url");
    }

    #[test]
    fn test_tool_result_with_image() {
        let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
        let compat = OpenAiCompat::openai();
        let config = StreamConfig {
            model: "gpt-4o".into(),
            system_prompt: String::new(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "call-1".into(),
                        name: "read_file".into(),
                        arguments: serde_json::json!({"path": "img.png"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                },
                Message::ToolResult {
                    tool_call_id: "call-1".into(),
                    tool_name: "read_file".into(),
                    content: vec![Content::Image {
                        data: "aW1hZ2VkYXRh".into(),
                        mime_type: "image/png".into(),
                    }],
                    is_error: false,
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test".into(),
            max_tokens: None,
            temperature: None,
            model_config: Some(model_config.clone()),
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, &model_config, &compat);
        let msgs = body["messages"].as_array().unwrap();
        let tool_msg = &msgs[1];
        assert_eq!(tool_msg["role"], "tool");
        let content = tool_msg["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "image_url");
    }

    #[test]
    fn test_tool_result_text_only_uses_string() {
        let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
        let compat = OpenAiCompat::openai();
        let config = StreamConfig {
            model: "gpt-4o".into(),
            system_prompt: String::new(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "call-1".into(),
                        name: "bash".into(),
                        arguments: serde_json::json!({"command": "echo hi"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                },
                Message::ToolResult {
                    tool_call_id: "call-1".into(),
                    tool_name: "bash".into(),
                    content: vec![Content::Text {
                        text: "hello".into(),
                    }],
                    is_error: false,
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test".into(),
            max_tokens: None,
            temperature: None,
            model_config: Some(model_config.clone()),
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, &model_config, &compat);
        let msgs = body["messages"].as_array().unwrap();
        let tool_msg = &msgs[1];
        assert_eq!(tool_msg["content"], "hello");
    }

    #[test]
    fn test_chunk_with_inline_error_parses_error_field() {
        use super::super::types::OpenAiChunk;
        let data = r#"{"choices":[],"error":{"message":"upstream failed"}}"#;
        let chunk: OpenAiChunk = serde_json::from_str(data).unwrap();
        assert!(chunk.error.is_some());
        assert_eq!(chunk.error.unwrap().message, "upstream failed");
    }

    #[test]
    fn test_chunk_without_error_has_none() {
        use super::super::types::OpenAiChunk;
        let data = r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#;
        let chunk: OpenAiChunk = serde_json::from_str(data).unwrap();
        assert!(chunk.error.is_none());
    }
}
