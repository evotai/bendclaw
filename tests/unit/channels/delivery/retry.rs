use bendclaw::channels::egress::retry::is_channel_retryable;
use bendclaw::channels::egress::retry::send_with_retry;
use bendclaw::channels::egress::retry::RetryConfig;
use bendclaw::types::ErrorCode;

#[test]
fn retryable_error_codes() {
    assert!(is_channel_retryable(&ErrorCode::timeout("timed out")));
    assert!(is_channel_retryable(&ErrorCode::internal("server error")));
    assert!(is_channel_retryable(&ErrorCode::channel_send(
        "send failed"
    )));
    assert!(is_channel_retryable(&ErrorCode::channel_timeout("slow")));
    assert!(is_channel_retryable(&ErrorCode::channel_rate_limited(
        "throttled"
    )));
}

#[test]
fn non_retryable_error_codes() {
    assert!(!is_channel_retryable(&ErrorCode::not_found("missing")));
    assert!(!is_channel_retryable(&ErrorCode::denied("forbidden")));
    assert!(!is_channel_retryable(&ErrorCode::invalid_input(
        "bad input"
    )));
}

#[test]
fn retryable_by_message_content() {
    assert!(is_channel_retryable(&ErrorCode::internal(
        "connection refused"
    )));
    assert!(is_channel_retryable(&ErrorCode::not_found(
        "timeout waiting"
    )));
    assert!(is_channel_retryable(&ErrorCode::denied("reset by peer")));
    assert!(is_channel_retryable(&ErrorCode::invalid_input(
        "HTTP 502 Bad Gateway"
    )));
    assert!(is_channel_retryable(&ErrorCode::invalid_input(
        "rate limit exceeded"
    )));
    assert!(is_channel_retryable(&ErrorCode::invalid_input(
        "too many requests"
    )));
}

#[test]
fn not_retryable_by_message() {
    assert!(!is_channel_retryable(&ErrorCode::not_found(
        "user not found"
    )));
    assert!(!is_channel_retryable(&ErrorCode::invalid_input(
        "invalid json"
    )));
}

#[tokio::test]
async fn send_with_retry_succeeds_immediately() {
    let config = RetryConfig {
        max_retries: 3,
        min_delay_ms: 10,
        max_delay_ms: 50,
    };
    let result = send_with_retry(|| async { Ok::<_, ErrorCode>(42) }, &config).await;
    assert_eq!(result.ok(), Some(42));
}

#[tokio::test]
async fn send_with_retry_retries_then_succeeds() {
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let config = RetryConfig {
        max_retries: 3,
        min_delay_ms: 10,
        max_delay_ms: 50,
    };
    let c = counter.clone();
    let result = send_with_retry(
        move || {
            let n = c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async move {
                if n < 2 {
                    Err(ErrorCode::channel_send("transient"))
                } else {
                    Ok(99)
                }
            }
        },
        &config,
    )
    .await;
    assert_eq!(result.ok(), Some(99));
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn send_with_retry_exhausts_retries() {
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let config = RetryConfig {
        max_retries: 2,
        min_delay_ms: 10,
        max_delay_ms: 50,
    };
    let c = counter.clone();
    let result: bendclaw::types::Result<i32> = send_with_retry(
        move || {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { Err(ErrorCode::channel_send("always fails")) }
        },
        &config,
    )
    .await;
    assert!(result.is_err());
    // 1 initial + 2 retries = 3 total attempts
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 3);
}

#[tokio::test]
async fn send_with_retry_no_retry_on_non_retryable() {
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let config = RetryConfig {
        max_retries: 3,
        min_delay_ms: 10,
        max_delay_ms: 50,
    };
    let c = counter.clone();
    let result: bendclaw::types::Result<i32> = send_with_retry(
        move || {
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            async { Err(ErrorCode::not_found("permanent")) }
        },
        &config,
    )
    .await;
    assert!(result.is_err());
    // Non-retryable: only 1 attempt, no retries.
    assert_eq!(counter.load(std::sync::atomic::Ordering::SeqCst), 1);
}
