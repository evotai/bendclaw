//! Anthropic request body building and content conversion.

use crate::provider::traits::StreamConfig;
use crate::types::*;

pub(crate) fn build_request_body(config: &StreamConfig, is_oauth: bool) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    for msg in &config.messages {
        match msg {
            Message::User { content, .. } => {
                messages.push(serde_json::json!({
                    "role": "user",
                    "content": content_to_anthropic(content),
                }));
            }
            Message::Assistant { content, .. } => {
                let blocks = content_to_anthropic(content);
                // Skip assistant messages with empty content — Anthropic rejects them
                // with "Improperly formed request". These can arise from empty provider
                // responses (e.g. proxy returning 200 with no actual content).
                if blocks.is_empty() {
                    continue;
                }
                messages.push(serde_json::json!({
                    "role": "assistant",
                    "content": blocks,
                }));
            }
            Message::ToolResult {
                tool_call_id,
                content,
                is_error,
                ..
            } => {
                let result_content = if content.iter().any(|c| matches!(c, Content::Image { .. })) {
                    // Multi-content with images: use array format
                    serde_json::json!(content_to_anthropic(content))
                } else {
                    // Text-only: use string shorthand
                    let text = content
                        .iter()
                        .find_map(|c| match c {
                            Content::Text { text } => Some(text.clone()),
                            _ => None,
                        })
                        .unwrap_or_default();
                    serde_json::json!(text)
                };

                messages.push(serde_json::json!({
                    "role": "user",
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": tool_call_id,
                        "content": result_content,
                        "is_error": is_error,
                    }],
                }));
            }
        }
    }

    // -----------------------------------------------------------------------
    // Prompt caching — place cache_control breakpoints based on CacheConfig.
    //
    // Anthropic caches the full prefix (tools → system → messages) up to each
    // breakpoint. We use up to 3 breakpoints:
    //   1. System prompt (stable across turns)
    //   2. Last tool definition (tools rarely change)
    //   3. Second-to-last message (conversation history grows, cache the prefix)
    //
    // When caching is disabled or strategy is Disabled, no markers are added.
    // -----------------------------------------------------------------------
    let cache = &config.cache_config;
    let caching_enabled = cache.enabled && cache.strategy != CacheStrategy::Disabled;
    let (cache_system, cache_tools, cache_messages) = match &cache.strategy {
        CacheStrategy::Auto => (true, true, true),
        CacheStrategy::Disabled => (false, false, false),
        CacheStrategy::Manual {
            cache_system,
            cache_tools,
            cache_messages,
        } => (*cache_system, *cache_tools, *cache_messages),
    };

    // Breakpoint 3: scan backwards from second-to-last message to find one with
    // non-empty content to place the cache breakpoint on
    if caching_enabled && cache_messages && messages.len() >= 2 {
        for idx in (0..messages.len() - 1).rev() {
            if let Some(content) = messages[idx]["content"].as_array_mut() {
                if let Some(last_block) = content.last_mut() {
                    let is_empty_text = last_block.get("type").and_then(|t| t.as_str())
                        == Some("text")
                        && last_block
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or("")
                            .is_empty();
                    if !is_empty_text {
                        last_block["cache_control"] = serde_json::json!({"type": "ephemeral"});
                        break;
                    }
                }
            }
        }
    }

    let mut body = serde_json::json!({
        "model": config.model,
        "max_tokens": config.max_tokens.unwrap_or(8192),
        "stream": true,
        "messages": messages,
    });

    // Breakpoint 1: system prompt
    if is_oauth {
        let mut system_blocks = vec![serde_json::json!({
            "type": "text",
            "text": "You are Claude Code, Anthropic's official CLI for Claude.",
        })];
        if !config.system_prompt.is_empty() {
            system_blocks.push(serde_json::json!({
                "type": "text",
                "text": config.system_prompt,
            }));
        }
        // Cache the last system block
        if caching_enabled && cache_system {
            if let Some(last) = system_blocks.last_mut() {
                last["cache_control"] = serde_json::json!({"type": "ephemeral"});
            }
        }
        body["system"] = serde_json::json!(system_blocks);
    } else if !config.system_prompt.is_empty() {
        let mut block = serde_json::json!({
            "type": "text",
            "text": config.system_prompt,
        });
        if caching_enabled && cache_system {
            block["cache_control"] = serde_json::json!({"type": "ephemeral"});
        }
        body["system"] = serde_json::json!([block]);
    }

    // Breakpoint 2: last tool definition (tools are stable between turns)
    if !config.tools.is_empty() {
        let mut tools: Vec<serde_json::Value> = config
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "description": t.description,
                    "input_schema": t.parameters,
                })
            })
            .collect();
        if caching_enabled && cache_tools {
            if let Some(last_tool) = tools.last_mut() {
                last_tool["cache_control"] = serde_json::json!({"type": "ephemeral"});
            }
        }
        body["tools"] = serde_json::json!(tools);
    }

    if config.thinking_level != ThinkingLevel::Off {
        let budget = match config.thinking_level {
            ThinkingLevel::Minimal => 128,
            ThinkingLevel::Low => 512,
            ThinkingLevel::Medium => 2048,
            ThinkingLevel::High => 8192,
            ThinkingLevel::Off => 0,
        };
        body["thinking"] = serde_json::json!({
            "type": "enabled",
            "budget_tokens": budget,
        });
    }

    if let Some(temp) = config.temperature {
        body["temperature"] = serde_json::json!(temp);
    }

    body
}

pub(crate) fn content_to_anthropic(content: &[Content]) -> Vec<serde_json::Value> {
    content
        .iter()
        .filter(|c| !matches!(c, Content::Text { text } if text.is_empty()))
        .map(|c| match c {
            Content::Text { text } => serde_json::json!({"type": "text", "text": text}),
            Content::Image { data, mime_type } => serde_json::json!({
                "type": "image",
                "source": {"type": "base64", "media_type": mime_type, "data": data},
            }),
            Content::Thinking {
                thinking,
                signature,
            } => serde_json::json!({
                "type": "thinking",
                "thinking": thinking,
                "signature": signature.as_deref().unwrap_or(""),
            }),
            Content::ToolCall {
                id,
                name,
                arguments,
            } => serde_json::json!({
                "type": "tool_use",
                "id": id,
                "name": name,
                "input": arguments,
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::traits::ToolDefinition;

    fn make_config(cache: CacheConfig) -> StreamConfig {
        StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![Message::user("Hello"), Message::User {
                content: vec![Content::Text {
                    text: "What is 2+2?".into(),
                }],
                timestamp: 0,
            }],
            tools: vec![ToolDefinition {
                name: "bash".into(),
                description: "Run commands".into(),
                parameters: serde_json::json!({"type": "object"}),
            }],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: cache,
        }
    }

    #[test]
    fn test_cache_auto_places_all_breakpoints() {
        let body = build_request_body(&make_config(CacheConfig::default()), false);

        // System prompt should have cache_control
        let system = &body["system"][0];
        assert_eq!(system["cache_control"]["type"], "ephemeral");

        // Last tool should have cache_control
        let tools = body["tools"].as_array().unwrap();
        let last_tool = tools.last().unwrap();
        assert_eq!(last_tool["cache_control"]["type"], "ephemeral");

        // Second-to-last message should have cache_control
        let msgs = body["messages"].as_array().unwrap();
        let second_to_last = &msgs[msgs.len() - 2];
        let content = second_to_last["content"].as_array().unwrap();
        let last_block = content.last().unwrap();
        assert_eq!(last_block["cache_control"]["type"], "ephemeral");
    }

    #[test]
    fn test_cache_disabled_no_breakpoints() {
        let config = CacheConfig {
            enabled: false,
            strategy: CacheStrategy::Auto,
        };
        let body = build_request_body(&make_config(config), false);

        let system = &body["system"][0];
        assert!(system.get("cache_control").is_none());

        let tools = body["tools"].as_array().unwrap();
        assert!(tools.last().unwrap().get("cache_control").is_none());

        let msgs = body["messages"].as_array().unwrap();
        for msg in msgs {
            if let Some(content) = msg["content"].as_array() {
                for block in content {
                    assert!(block.get("cache_control").is_none());
                }
            }
        }
    }

    #[test]
    fn test_cache_manual_system_only() {
        let config = CacheConfig {
            enabled: true,
            strategy: CacheStrategy::Manual {
                cache_system: true,
                cache_tools: false,
                cache_messages: false,
            },
        };
        let body = build_request_body(&make_config(config), false);

        assert_eq!(body["system"][0]["cache_control"]["type"], "ephemeral");
        assert!(body["tools"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .get("cache_control")
            .is_none());
        let msgs = body["messages"].as_array().unwrap();
        let second = &msgs[msgs.len() - 2];
        let content = second["content"].as_array().unwrap();
        assert!(content.last().unwrap().get("cache_control").is_none());
    }

    #[test]
    fn test_usage_cache_hit_rate() {
        let usage = Usage {
            input: 100,
            output: 50,
            cache_read: 900,
            cache_write: 0,
            total_tokens: 1050,
        };
        let rate = usage.cache_hit_rate();
        assert!((rate - 0.9).abs() < 0.001);

        let empty = Usage::default();
        assert_eq!(empty.cache_hit_rate(), 0.0);
    }

    #[test]
    fn test_tool_result_with_image() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "".into(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "tc-1".into(),
                        name: "read_file".into(),
                        arguments: serde_json::json!({"path": "test.png"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                    model: "test".into(),
                    provider: "test".into(),
                    usage: Usage::default(),
                    timestamp: 0,
                    error_message: None,
                },
                Message::ToolResult {
                    tool_call_id: "tc-1".into(),
                    tool_name: "read_file".into(),
                    content: vec![
                        Content::Text {
                            text: "screenshot".into(),
                        },
                        Content::Image {
                            data: "aW1hZ2VkYXRh".into(),
                            mime_type: "image/png".into(),
                        },
                    ],
                    is_error: false,
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig {
                enabled: false,
                strategy: CacheStrategy::Disabled,
            },
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        let tool_msg = &msgs[1];
        let tool_result = &tool_msg["content"][0];
        assert_eq!(tool_result["type"], "tool_result");
        let content = tool_result["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
    }

    #[test]
    fn test_tool_result_text_only_uses_string() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "".into(),
            messages: vec![
                Message::Assistant {
                    content: vec![Content::ToolCall {
                        id: "tc-1".into(),
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
                    tool_call_id: "tc-1".into(),
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
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig {
                enabled: false,
                strategy: CacheStrategy::Disabled,
            },
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        let tool_result = &msgs[1]["content"][0];
        assert_eq!(tool_result["content"], "hello");
    }

    #[test]
    fn test_content_to_anthropic_filters_empty_text() {
        let content = vec![
            Content::Text { text: "".into() },
            Content::Text {
                text: "hello".into(),
            },
            Content::Text { text: "".into() },
        ];
        let result = content_to_anthropic(&content);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0]["text"], "hello");
    }

    #[test]
    fn test_cache_control_not_set_on_empty_text_block() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![
                Message::User {
                    content: vec![Content::Text {
                        text: "first message".into(),
                    }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text { text: "".into() }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text {
                        text: "last".into(),
                    }],
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        };
        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();
        let second_to_last = &msgs[msgs.len() - 2];
        let content = second_to_last["content"].as_array().unwrap();
        assert!(
            content.is_empty(),
            "empty text blocks should be filtered out"
        );

        let first = &msgs[0];
        let first_content = first["content"].as_array().unwrap();
        let last_block = first_content.last().unwrap();
        assert_eq!(
            last_block["cache_control"]["type"], "ephemeral",
            "cache_control should fall back to an earlier message with content"
        );
    }

    #[test]
    fn test_cache_breakpoint_falls_back_when_second_to_last_is_empty() {
        let config = StreamConfig {
            model: "claude-sonnet-4-20250514".into(),
            system_prompt: "You are helpful.".into(),
            messages: vec![
                Message::User {
                    content: vec![Content::Text {
                        text: "first message".into(),
                    }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text { text: "".into() }],
                    timestamp: 0,
                },
                Message::User {
                    content: vec![Content::Text {
                        text: "last message".into(),
                    }],
                    timestamp: 0,
                },
            ],
            tools: vec![],
            thinking_level: ThinkingLevel::Off,
            api_key: "test-key".into(),
            max_tokens: Some(1024),
            temperature: None,
            model_config: None,
            cache_config: CacheConfig::default(),
        };

        let body = build_request_body(&config, false);
        let msgs = body["messages"].as_array().unwrap();

        let first_content = msgs[0]["content"].as_array().unwrap();
        assert_eq!(
            first_content.last().unwrap()["cache_control"]["type"],
            "ephemeral"
        );
    }
}
