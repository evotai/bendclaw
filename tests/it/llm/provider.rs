use bendclaw::llm::provider::mask_api_key;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::tool::FunctionDef;
use bendclaw::llm::tool::ToolSchema;

#[test]
fn mask_api_key_short() {
    assert_eq!(mask_api_key("abc"), "***");
}

#[test]
fn mask_api_key_exact_four() {
    assert_eq!(mask_api_key("abcd"), "****");
}

#[test]
fn mask_api_key_longer() {
    assert_eq!(mask_api_key("sk-1234567890"), "***7890");
}

#[test]
fn mask_api_key_empty() {
    assert_eq!(mask_api_key(""), "");
}

#[test]
fn llm_response_has_tool_calls_empty() {
    let r = LLMResponse {
        content: Some("hello".into()),
        tool_calls: vec![],
        finish_reason: Some("stop".into()),
        usage: None,
        model: None,
    };
    assert!(!r.has_tool_calls());
}

#[test]
fn llm_response_has_tool_calls_nonempty() {
    let r = LLMResponse {
        content: None,
        tool_calls: vec![bendclaw::llm::message::ToolCall {
            id: "tc1".into(),
            name: "shell".into(),
            arguments: "{}".into(),
        }],
        finish_reason: None,
        usage: None,
        model: None,
    };
    assert!(r.has_tool_calls());
}

#[test]
fn tool_schema_new() {
    let schema = ToolSchema::new("grep", "search files", serde_json::json!({}));
    assert_eq!(schema.schema_type, "function");
    assert_eq!(schema.function.name, "grep");
    assert_eq!(schema.function.description, "search files");
}

#[test]
fn tool_schema_serde_roundtrip() {
    let schema = ToolSchema::new("test", "desc", serde_json::json!({"type": "object"}));
    let json = serde_json::to_string(&schema).unwrap();
    let back: ToolSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(back.function.name, "test");
    assert_eq!(back.schema_type, "function");
}

#[test]
fn function_def_serde_roundtrip() {
    let def = FunctionDef {
        name: "fn1".into(),
        description: "does stuff".into(),
        parameters: serde_json::json!({}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let back: FunctionDef = serde_json::from_str(&json).unwrap();
    assert_eq!(back.name, "fn1");
}
