use bendengine::provider::google::*;
use bendengine::provider::traits::*;
use bendengine::types::*;

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
