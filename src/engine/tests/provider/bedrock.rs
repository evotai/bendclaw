use bendengine::provider::bedrock::*;
use bendengine::provider::traits::*;
use bendengine::types::*;

#[test]
fn test_build_bedrock_body() {
    let config = StreamConfig {
        model: "anthropic.claude-3-sonnet-20240229-v1:0".into(),
        system_prompt: "Be helpful".into(),
        messages: vec![Message::user("Hello")],
        tools: vec![],
        thinking_level: ThinkingLevel::Off,
        api_key: "key:secret".into(),
        max_tokens: Some(1024),
        temperature: None,
        model_config: None,
        cache_config: CacheConfig::default(),
    };

    let body = build_bedrock_body(&config);
    assert!(body["messages"].is_array());
    assert_eq!(body["messages"][0]["role"], "user");
    assert!(body["system"].is_array());
    assert_eq!(body["inferenceConfig"]["maxTokens"], 1024);
}

#[test]
fn test_content_to_bedrock_filters_empty_text() {
    let content = vec![
        Content::Text { text: "".into() },
        Content::Text {
            text: "hello".into(),
        },
        Content::Text { text: "".into() },
    ];
    let blocks = content_to_bedrock(&content);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0]["text"], "hello");
}

#[test]
fn test_content_to_bedrock() {
    let content = vec![
        Content::Text {
            text: "hello".into(),
        },
        Content::ToolCall {
            id: "tc-1".into(),
            name: "bash".into(),
            arguments: serde_json::json!({"command": "ls"}),
        },
    ];
    let blocks = content_to_bedrock(&content);
    assert_eq!(blocks.len(), 2);
    assert_eq!(blocks[0]["text"], "hello");
    assert_eq!(blocks[1]["toolUse"]["name"], "bash");
}
