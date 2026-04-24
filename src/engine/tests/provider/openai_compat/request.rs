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
    assert_eq!(body["messages"][0]["role"], "developer");
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
