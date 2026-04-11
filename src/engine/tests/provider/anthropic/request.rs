use bendengine::provider::anthropic::request::*;
use bendengine::provider::traits::*;
use bendengine::types::*;

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

    let system = &body["system"][0];
    assert_eq!(system["cache_control"]["type"], "ephemeral");

    let tools = body["tools"].as_array().unwrap();
    let last_tool = tools.last().unwrap();
    assert_eq!(last_tool["cache_control"]["type"], "ephemeral");

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
