//! OpenAI-compatible request body building and content conversion.

use crate::provider::model::CompatCaps;
use crate::provider::model::MaxTokensField;
use crate::provider::model::ModelConfig;
use crate::provider::model::OpenAiCompat;
use crate::provider::traits::StreamConfig;
use crate::types::*;

#[derive(Default)]
pub struct ToolCallBuffer {
    pub id: String,
    pub name: String,
    pub arguments: String,
}

pub fn build_request_body(
    config: &StreamConfig,
    model_config: &ModelConfig,
    compat: &OpenAiCompat,
) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // System prompt
    if !config.system_prompt.is_empty() {
        messages.push(serde_json::json!({
            "role": "system",
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
                let mut reasoning = String::new();

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
                        Content::Thinking { thinking, .. } => {
                            reasoning.push_str(thinking);
                        }
                        _ => {}
                    }
                }

                // Skip empty assistant messages that have neither content nor tool_calls
                // nor reasoning_content.
                // Some providers (e.g. mimo-v2.5-pro) reject assistant messages without
                // at least one of content, reasoning_content, or tool_calls.
                if parts.is_empty() && tool_calls.is_empty() && reasoning.is_empty() {
                    continue;
                }

                let mut msg_obj = serde_json::json!({"role": "assistant"});
                if !parts.is_empty() {
                    msg_obj["content"] = serde_json::json!(parts);
                }
                if !tool_calls.is_empty() {
                    msg_obj["tool_calls"] = serde_json::json!(tool_calls);
                }
                apply_assistant_compat(&mut msg_obj, compat, &reasoning);
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
                apply_tool_result_compat(&mut msg_obj, compat, tool_name);
                messages.push(msg_obj);
            }
        }
    }

    let max_tokens_val = config.max_tokens.unwrap_or(model_config.max_tokens);
    let mut body = serde_json::json!({
        "model": config.model,
        "stream": true,
        "messages": messages,
    });

    if compat.has_cap(CompatCaps::USAGE_IN_STREAMING) {
        body["stream_options"] = serde_json::json!({"include_usage": true});
    }

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

    apply_reasoning_effort(&mut body, config, compat);

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

fn apply_assistant_compat(msg_obj: &mut serde_json::Value, compat: &OpenAiCompat, reasoning: &str) {
    if !reasoning.is_empty() || compat.has_cap(CompatCaps::REASONING_CONTENT_REQUIRED) {
        msg_obj["reasoning_content"] = serde_json::json!(reasoning);
    }
}

fn apply_tool_result_compat(
    msg_obj: &mut serde_json::Value,
    compat: &OpenAiCompat,
    tool_name: &str,
) {
    if compat.has_cap(CompatCaps::TOOL_RESULT_NAME) {
        msg_obj["name"] = serde_json::json!(tool_name);
    }
}

fn apply_reasoning_effort(
    body: &mut serde_json::Value,
    config: &StreamConfig,
    compat: &OpenAiCompat,
) {
    match config.thinking_level {
        ThinkingLevel::Off => {}
        level if compat.has_cap(CompatCaps::REASONING_EFFORT) => {
            let effort = match level {
                ThinkingLevel::Minimal | ThinkingLevel::Low => "low",
                ThinkingLevel::Medium => "medium",
                ThinkingLevel::High | ThinkingLevel::Adaptive => "high",
                ThinkingLevel::Off => unreachable!(),
            };
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        _ => {}
    }
}

pub fn content_to_openai(content: &[Content]) -> serde_json::Value {
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
