//! Tests for shared stream HTTP helpers.

use bendengine::provider::stream_http::classify_json_error;
use bendengine::provider::stream_http::extract_json_error_message;
use bendengine::provider::stream_http::StreamResponseKind;
use bendengine::provider::ProviderError;

// ---------------------------------------------------------------------------
// extract_json_error_message
// ---------------------------------------------------------------------------

#[test]
fn extract_anthropic_error_message() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("overloaded_error: Overloaded".into()));
}

#[test]
fn extract_openai_error_message() {
    let value = serde_json::json!({
        "error": {
            "message": "server error"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("server error".into()));
}

#[test]
fn extract_generic_message_field() {
    let value = serde_json::json!({
        "message": "internal error"
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("internal error".into()));
}

#[test]
fn extract_type_only() {
    let value = serde_json::json!({
        "type": "rate_limit_error"
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("rate_limit_error".into()));
}

#[test]
fn extract_no_known_fields() {
    let value = serde_json::json!({"foo": "bar"});
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, None);
}

// ---------------------------------------------------------------------------
// classify_json_error
// ---------------------------------------------------------------------------

#[test]
fn classify_overflow_json() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "prompt is too long: 213462 tokens > 200000 maximum"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!err.is_retryable());
}

#[test]
fn classify_generic_json_error_is_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "api_error",
            "message": "Internal server error"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(err.is_retryable());
}

#[test]
fn classify_overloaded_json_is_retryable() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(err.is_retryable());
}

#[test]
fn classify_no_message_uses_full_json() {
    let value = serde_json::json!({"foo": "bar"});
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(err.is_retryable());
}

// ---------------------------------------------------------------------------
// StreamResponseKind (via classify_response — tested indirectly through
// the public enum since classify_response takes a reqwest::Response)
// ---------------------------------------------------------------------------

#[test]
fn stream_response_kind_variants() {
    // Just verify the enum is usable
    let streaming = StreamResponseKind::Streaming;
    let json = StreamResponseKind::Json;
    let other = StreamResponseKind::Other("text/plain".into());

    assert_eq!(streaming, StreamResponseKind::Streaming);
    assert_eq!(json, StreamResponseKind::Json);
    assert!(matches!(other, StreamResponseKind::Other(_)));
}
