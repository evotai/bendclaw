use evotengine::provider::model::ModelConfig;
use evotengine::provider::model::OpenAiCompat;
use evotengine::provider::openai_compat::request::*;
use evotengine::provider::openai_compat::types::OpenAiChunk;
use evotengine::types::*;

use super::super::fixtures::stream_config::*;

#[test]
fn test_adaptive_thinking_maps_to_high_reasoning_effort() {
    let model_config = ModelConfig::openai("gpt-5", "GPT-5");
    let config = StreamConfigBuilder::openai()
        .model("gpt-5")
        .thinking(ThinkingLevel::Adaptive)
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    assert_eq!(body["reasoning_effort"], "high");
}

#[test]
fn test_medium_thinking_maps_to_medium_reasoning_effort() {
    let model_config = ModelConfig::openai("gpt-5", "GPT-5");
    let config = StreamConfigBuilder::openai()
        .model("gpt-5")
        .thinking(ThinkingLevel::Medium)
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    assert_eq!(body["reasoning_effort"], "medium");
}

#[test]
fn test_low_thinking_maps_to_low_reasoning_effort() {
    let model_config = ModelConfig::openai("gpt-5", "GPT-5");
    let config = StreamConfigBuilder::openai()
        .model("gpt-5")
        .thinking(ThinkingLevel::Low)
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    assert_eq!(body["reasoning_effort"], "low");
}

#[test]
fn test_off_thinking_omits_reasoning_effort() {
    let model_config = ModelConfig::openai("gpt-5", "GPT-5");
    let config = StreamConfigBuilder::openai()
        .model("gpt-5")
        .thinking(ThinkingLevel::Off)
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_compat_without_reasoning_support_omits_reasoning_effort() {
    let model_config = ModelConfig::openai("gpt-5", "GPT-5");
    let config = StreamConfigBuilder::openai()
        .model("gpt-5")
        .thinking(ThinkingLevel::Adaptive)
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::default());
    assert!(body.get("reasoning_effort").is_none());
}

#[test]
fn test_build_request_body_basic() {
    let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
    let config = StreamConfigBuilder::openai()
        .system_prompt("You are helpful.")
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    assert_eq!(body["model"], "gpt-4o");
    assert!(body["stream"].as_bool().unwrap());
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["messages"][1]["role"], "user");
    assert!(body["max_completion_tokens"].is_number());
}

#[test]
fn test_build_request_body_with_tools() {
    let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
    let compat = OpenAiCompat::openai();
    let config = StreamConfigBuilder::openai()
        .messages(vec![Message::user("List files")])
        .tools(vec![tool_def("bash", "Run a command")])
        .max_tokens(1024)
        .temperature(0.5)
        .build();

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
    let config = StreamConfigBuilder::openai()
        .messages(vec![
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
                retention: Retention::Normal,
            },
        ])
        .build();

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
    let config = StreamConfigBuilder::openai()
        .messages(vec![
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
                retention: Retention::Normal,
            },
        ])
        .build();

    let body = build_request_body(&config, &model_config, &compat);
    let msgs = body["messages"].as_array().unwrap();
    let tool_msg = &msgs[1];
    assert_eq!(tool_msg["content"], "hello");
}

#[test]
fn test_empty_assistant_message_is_skipped() {
    let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
    let compat = OpenAiCompat::openai();
    let config = StreamConfigBuilder::openai()
        .messages(vec![
            Message::user("hello"),
            Message::Assistant {
                content: vec![Content::Text { text: "".into() }],
                stop_reason: StopReason::Stop,
                model: "test".into(),
                provider: "test".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
            },
            Message::user("world"),
        ])
        .build();

    let body = build_request_body(&config, &model_config, &compat);
    let msgs = body["messages"].as_array().unwrap();
    // user("hello") + user("world") = 2, empty assistant skipped (no system prompt)
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0]["role"], "user");
    assert_eq!(msgs[1]["role"], "user");
    // No message should have role "assistant" with missing content
    for msg in msgs {
        if msg["role"] == "assistant" {
            assert!(
                msg.get("content").is_some()
                    || msg.get("tool_calls").is_some()
                    || msg.get("reasoning_content").is_some(),
                "assistant message must have content or tool_calls"
            );
        }
    }
}

#[test]
fn test_chunk_with_inline_error_parses_error_field() {
    let data = r#"{"choices":[],"error":{"message":"upstream failed"}}"#;
    let chunk: OpenAiChunk = serde_json::from_str(data).unwrap();
    assert!(chunk.error.is_some());
    assert_eq!(chunk.error.unwrap().message, "upstream failed");
}

#[test]
fn test_chunk_without_error_has_none() {
    let data = r#"{"choices":[{"delta":{"content":"hi"},"finish_reason":null}]}"#;
    let chunk: OpenAiChunk = serde_json::from_str(data).unwrap();
    assert!(chunk.error.is_none());
}

#[test]
fn test_reasoning_content_in_request() {
    let model_config = ModelConfig::openai("deepseek-v4-pro", "DeepSeek V4 Pro");
    let config = StreamConfigBuilder::openai()
        .model("deepseek-v4-pro")
        .messages(vec![
            Message::user("hello"),
            Message::Assistant {
                content: vec![
                    Content::Thinking {
                        thinking: "Let me think about this...".into(),
                        signature: None,
                    },
                    Content::Text {
                        text: "Here is the answer.".into(),
                    },
                ],
                stop_reason: StopReason::Stop,
                model: "deepseek-v4-pro".into(),
                provider: "deepseek".into(),
                usage: Usage::default(),
                timestamp: 0,
                error_message: None,
            },
            Message::user("thanks"),
        ])
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    let msgs = body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 3);
    let asst = &msgs[1];
    assert_eq!(asst["role"], "assistant");
    assert_eq!(asst["reasoning_content"], "Let me think about this...");
    assert!(asst["content"].is_array());
}

#[test]
fn test_thinking_only_assistant_not_skipped() {
    let model_config = ModelConfig::openai("deepseek-v4-pro", "DeepSeek V4 Pro");
    let config = StreamConfigBuilder::openai()
        .model("deepseek-v4-pro")
        .messages(vec![Message::user("test"), Message::Assistant {
            content: vec![Content::Thinking {
                thinking: "internal reasoning only".into(),
                signature: None,
            }],
            stop_reason: StopReason::Stop,
            model: "deepseek-v4-pro".into(),
            provider: "deepseek".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }])
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::openai());
    let msgs = body["messages"].as_array().unwrap();
    // user + assistant (thinking only, NOT skipped) = 2
    assert_eq!(
        msgs.len(),
        2,
        "assistant with only thinking should not be skipped"
    );
    let asst = &msgs[1];
    assert_eq!(asst["role"], "assistant");
    assert_eq!(asst["reasoning_content"], "internal reasoning only");
    assert!(asst.get("content").is_none());
}

#[test]
fn test_tool_call_assistant_includes_empty_reasoning_content() {
    let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
    let config = StreamConfigBuilder::openai()
        .messages(vec![Message::user("test"), Message::Assistant {
            content: vec![Content::ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/a"}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "claude-opus-4-6".into(),
            provider: "anthropic".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }])
        .build();

    let body = build_request_body(&config, &model_config, &OpenAiCompat::deepseek());
    let msgs = body["messages"].as_array().unwrap();
    let asst = &msgs[1];
    assert_eq!(asst["role"], "assistant");
    assert_eq!(asst["reasoning_content"], "");
    assert!(asst["tool_calls"].is_array());
}

#[test]
fn test_tool_call_assistant_omits_empty_reasoning_content_without_cap() {
    let model_config = ModelConfig::openai("gpt-4o", "GPT-4o");
    let config = StreamConfigBuilder::openai()
        .messages(vec![Message::user("test"), Message::Assistant {
            content: vec![Content::ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "/tmp/a"}),
            }],
            stop_reason: StopReason::ToolUse,
            model: "claude-opus-4-6".into(),
            provider: "anthropic".into(),
            usage: Usage::default(),
            timestamp: 0,
            error_message: None,
        }])
        .build();

    let compat = OpenAiCompat::openai();
    // OpenAI doesn't have this cap by default, so no need to remove it.
    let body = build_request_body(&config, &model_config, &compat);
    let msgs = body["messages"].as_array().unwrap();
    let asst = &msgs[1];
    assert_eq!(asst["role"], "assistant");
    assert!(asst.get("reasoning_content").is_none());
    assert!(asst["tool_calls"].is_array());
}
