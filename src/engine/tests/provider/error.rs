use evotengine::provider::error::*;

#[test]
fn classify_anthropic_overflow() {
    let err = ProviderError::classify(
        400,
        "prompt is too long: 213462 tokens > 200000 maximum",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_openai_overflow() {
    let err = ProviderError::classify(
        400,
        "Your input exceeds the context window of this model",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_google_overflow() {
    let err = ProviderError::classify(
        400,
        "The input token count (1196265) exceeds the maximum number of tokens allowed",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_bedrock_overflow() {
    let err = ProviderError::classify(400, "input is too long for requested model", None);
    assert!(err.is_context_overflow());
}

#[test]
fn classify_xai_overflow() {
    let err = ProviderError::classify(
        400,
        "This model's maximum prompt length is 131072 but request contains 537812 tokens",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_groq_overflow() {
    let err = ProviderError::classify(
        400,
        "Please reduce the length of the messages or completion",
        None,
    );
    assert!(err.is_context_overflow());
}

#[test]
fn classify_empty_body_overflow() {
    let err = ProviderError::classify(413, "", None);
    assert!(err.is_context_overflow());
    let err = ProviderError::classify(400, "  ", None);
    assert!(err.is_context_overflow());
}

#[test]
fn classify_rate_limit() {
    let err = ProviderError::classify(429, "rate limit exceeded", None);
    assert!(matches!(err, ProviderError::RateLimited { .. }));
}

#[test]
fn classify_rate_limit_with_retry_after() {
    let err = ProviderError::classify(429, "rate limit exceeded", Some(5000));
    match err {
        ProviderError::RateLimited { retry_after_ms } => {
            assert_eq!(retry_after_ms, Some(5000));
        }
        _ => panic!("Expected RateLimited"),
    }
}

#[test]
fn classify_auth_error() {
    let err = ProviderError::classify(401, "invalid api key", None);
    assert!(matches!(err, ProviderError::Auth(_)));
    assert!(!evotengine::retry::should_retry(&err));
    let err = ProviderError::classify(403, "forbidden", None);
    assert!(matches!(err, ProviderError::Auth(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_400_not_retryable() {
    let err = ProviderError::classify(400, "invalid request format", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_529_overloaded() {
    let err = ProviderError::classify(529, "overloaded", None);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn classify_sse_overloaded_error() {
    let err = classify_sse_error_event(r#"{"type":"overloaded_error","message":"Overloaded"}"#);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(evotengine::retry::should_retry(&err));
}

#[test]
fn overflow_message_case_insensitive() {
    assert!(is_context_overflow_message("PROMPT IS TOO LONG"));
    assert!(is_context_overflow_message("Too Many Tokens in request"));
}

#[test]
fn non_overflow_messages() {
    assert!(!is_context_overflow_message("invalid api key"));
    assert!(!is_context_overflow_message("internal server error"));
    assert!(!is_context_overflow_message(""));
}

#[test]
fn classify_404_not_retryable() {
    let err = ProviderError::classify(404, "model not found", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_405_not_retryable() {
    let err = ProviderError::classify(405, "method not allowed", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}

#[test]
fn classify_422_not_retryable() {
    let err = ProviderError::classify(422, "unprocessable entity", None);
    assert!(matches!(err, ProviderError::Other(_)));
    assert!(!evotengine::retry::should_retry(&err));
}
