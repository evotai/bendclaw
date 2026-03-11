use anyhow::Result;
use bendclaw::llm::provider::mask_api_key;
use bendclaw::llm::provider::response_headers_value;
use bendclaw::llm::provider::response_request_id;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::tool::FunctionDef;
use bendclaw::llm::tool::ToolSchema;
use reqwest::header::HeaderMap;
use reqwest::header::HeaderName;
use reqwest::header::HeaderValue;

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
fn tool_schema_serde_roundtrip() -> Result<()> {
    let schema = ToolSchema::new("test", "desc", serde_json::json!({"type": "object"}));
    let json = serde_json::to_string(&schema)?;
    let back: ToolSchema = serde_json::from_str(&json)?;
    assert_eq!(back.function.name, "test");
    assert_eq!(back.schema_type, "function");
    Ok(())
}

#[test]
fn function_def_serde_roundtrip() -> Result<()> {
    let def = FunctionDef {
        name: "fn1".into(),
        description: "does stuff".into(),
        parameters: serde_json::json!({}),
    };
    let json = serde_json::to_string(&def)?;
    let back: FunctionDef = serde_json::from_str(&json)?;
    assert_eq!(back.name, "fn1");
    Ok(())
}

// ── response_headers_value ──

#[test]
fn response_headers_value_empty_map() {
    let headers = HeaderMap::new();
    let v = response_headers_value(&headers);
    assert!(v.is_object());
    assert!(v.as_object().unwrap().is_empty());
}

#[test]
fn response_headers_value_single_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("application/json"),
    );
    let v = response_headers_value(&headers);
    assert_eq!(v["content-type"], "application/json");
}

#[test]
fn response_headers_value_multiple_headers() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req-123"),
    );
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/plain"),
    );
    let v = response_headers_value(&headers);
    assert_eq!(v["x-request-id"], "req-123");
    assert_eq!(v["content-type"], "text/plain");
}

// ── response_request_id ──

#[test]
fn response_request_id_empty_headers_returns_empty() {
    let headers = HeaderMap::new();
    assert_eq!(response_request_id(&headers), "");
}

#[test]
fn response_request_id_x_request_id() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("req-abc"),
    );
    assert_eq!(response_request_id(&headers), "req-abc");
}

#[test]
fn response_request_id_openai_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("openai-request-id"),
        HeaderValue::from_static("openai-xyz"),
    );
    assert_eq!(response_request_id(&headers), "openai-xyz");
}

#[test]
fn response_request_id_anthropic_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("anthropic-request-id"),
        HeaderValue::from_static("ant-456"),
    );
    assert_eq!(response_request_id(&headers), "ant-456");
}

#[test]
fn response_request_id_x_amzn_requestid() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-amzn-requestid"),
        HeaderValue::from_static("amzn-789"),
    );
    assert_eq!(response_request_id(&headers), "amzn-789");
}

#[test]
fn response_request_id_request_id_header() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("request-id"),
        HeaderValue::from_static("rid-001"),
    );
    assert_eq!(response_request_id(&headers), "rid-001");
}

#[test]
fn response_request_id_prefers_x_request_id_first() {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("x-request-id"),
        HeaderValue::from_static("first"),
    );
    headers.insert(
        HeaderName::from_static("openai-request-id"),
        HeaderValue::from_static("second"),
    );
    assert_eq!(response_request_id(&headers), "first");
}
