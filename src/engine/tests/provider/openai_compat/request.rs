use bendengine::provider::model::ModelConfig;
use bendengine::provider::model::OpenAiCompat;
use bendengine::provider::openai_compat::request::*;
use bendengine::provider::openai_compat::types::OpenAiChunk;
use bendengine::provider::traits::*;
use bendengine::types::*;

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
    assert_eq!(body["messages"][0]["role"], "developer");
    assert_eq!(body["messages"][1]["role"], "user");
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
