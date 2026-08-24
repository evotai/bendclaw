//! Tests for shared stream HTTP helpers.

use evotengine::provider::stream_http::check_error_status;
use evotengine::provider::stream_http::classify_json_error;
use evotengine::provider::stream_http::extract_json_error_message;
use evotengine::provider::stream_http::StreamResponseKind;
use evotengine::provider::ProviderError;

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
fn extract_openai_error_code_only() {
    let value = serde_json::json!({
        "error": {
            "code": "insufficient_quota",
            "message": "You exceeded your current quota"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(
        msg,
        Some("insufficient_quota: You exceeded your current quota".into())
    );
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

#[test]
fn extract_openrouter_metadata_raw_json() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": "Provider returned error"
        },
        "metadata": {
            "raw": "{\"error\":{\"type\":\"invalid_request_error\",\"message\":\"prompt is too long\"}}",
            "provider_name": "Stealth"
        }
    });
    let msg = extract_json_error_message(&value).expect("message");
    assert!(msg.starts_with("invalid_request_error: Provider returned error"));
    assert!(msg.contains("prompt is too long"));
    assert_eq!(msg.matches("prompt is too long").count(), 1);
}

#[test]
fn extract_openrouter_metadata_raw_plain_text() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "Provider returned error",
            "metadata": {"raw": "max_tokens too large"}
        }
    });
    let msg = extract_json_error_message(&value).expect("message");
    assert!(msg.starts_with("invalid_request_error: Provider returned error"));
    assert!(msg.contains("max_tokens too large"));
    assert_eq!(msg.matches("max_tokens too large").count(), 1);
}

#[test]
fn extract_does_not_duplicate_raw_already_in_the_message() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "Unsupported tool schema"
        },
        "metadata": {"raw": "Unsupported tool schema"}
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(
        msg,
        Some("invalid_request_error: Unsupported tool schema".into())
    );
}

#[test]
fn extract_opaque_raw_is_still_appended() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "Provider returned error"
        },
        "metadata": {"raw": "ERROR", "provider_name": "Stealth"}
    });
    let msg = extract_json_error_message(&value).expect("message");
    assert!(msg.contains("Provider returned error"));
    assert!(msg.contains("ERROR"));
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
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_internal_server_error_json_is_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "api_error",
            "message": "Internal server error"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_overloaded_json_is_retryable() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "service is overloaded"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Overloaded(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_no_message_uses_full_json() {
    // No recognizable error fields: the payload still arrived on an accepted
    // (2xx) request, so it defaults to a retryable transient error rather
    // than failing hard on unknown shapes.
    let value = serde_json::json!({"foo": "bar"});
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_json_404_is_not_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "not_found_error",
            "message": "model not found"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_json_400_bad_request_is_not_retryable() {
    let value = serde_json::json!({
        "error": {
            "type": "invalid_request_error",
            "message": "Bad request: missing required parameter text"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_openrouter_wrapped_overflow_from_metadata_raw() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": "Provider returned error"
        },
        "metadata": {
            "raw": "{\"error\":{\"message\":\"prompt is too long: 213462 tokens > 200000 maximum\"}}",
            "provider_name": "Stealth"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!evotengine::retry::should_retry(&err));
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

// ---------------------------------------------------------------------------
// check_error_status — gateway hint headers
// ---------------------------------------------------------------------------

async fn error_from_mock_response(template: wiremock::ResponseTemplate) -> ProviderError {
    use wiremock::matchers::method;
    use wiremock::Mock;
    use wiremock::MockServer;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(template)
        .mount(&server)
        .await;
    let response = match reqwest::get(server.uri()).await {
        Ok(response) => response,
        Err(error) => panic!("mock request failed: {error}"),
    };
    match check_error_status(response).await {
        Err(error) => error,
        Ok(_) => panic!("expected an error classification"),
    }
}

#[tokio::test]
async fn check_error_status_honors_should_retry() {
    // A gateway can mark an unknown 4xx as retryable.
    let err = error_from_mock_response(
        wiremock::ResponseTemplate::new(402)
            .insert_header("x-should-retry", "true")
            .set_body_string("payment required"),
    )
    .await;
    assert!(matches!(err, ProviderError::Transient { .. }));
    assert!(evotengine::retry::should_retry(&err));
}

#[tokio::test]
async fn check_error_status_preserves_nested_rate_limit_detail() {
    let detail = "Rate limit exceeded. Please retry later.";
    let err = error_from_mock_response(wiremock::ResponseTemplate::new(429).set_body_json(
        serde_json::json!({
            "type": "error",
            "error": {"type": "rate_limit_error", "message": detail}
        }),
    ))
    .await;
    match err {
        ProviderError::RateLimited { message } => {
            assert_eq!(
                message,
                "HTTP 429: rate_limit_error: Rate limit exceeded. Please retry later."
            );
        }
        other => panic!("expected RateLimited, got {other:?}"),
    }
}

#[tokio::test]
async fn check_error_status_classifies_code_only_quota_and_overflow() {
    let fatal = error_from_mock_response(wiremock::ResponseTemplate::new(429).set_body_json(
        serde_json::json!({
            "error": {"code": "insufficient_quota", "message": "You exceeded your current quota"}
        }),
    ))
    .await;
    assert!(matches!(fatal, ProviderError::Other(_)));
    assert!(!fatal.is_quota_limited());
    assert!(!evotengine::retry::should_retry(&fatal));
    assert!(fatal.to_string().contains("insufficient_quota"));

    let overflow = error_from_mock_response(wiremock::ResponseTemplate::new(400).set_body_json(
        serde_json::json!({
            "error": {"code": "context_length_exceeded", "message": "too many tokens in request"}
        }),
    ))
    .await;
    assert!(overflow.is_context_overflow());
    assert!(!evotengine::retry::should_retry(&overflow));
}

#[tokio::test]
async fn check_error_status_truncates_oversized_bodies() {
    let oversized = error_from_mock_response(wiremock::ResponseTemplate::new(429).set_body_string(
        format!("{{\"error\":{{\"message\":\"{}\"}}}}", "x".repeat(80_000)),
    ))
    .await;

    assert!(oversized.to_string().contains('…'));
    assert!(oversized.to_string().len() < 80_000);
}
