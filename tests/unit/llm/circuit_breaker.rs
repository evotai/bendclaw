use std::time::Duration;

use anyhow::Result;
use bendclaw::llm::circuit_breaker::is_transient;
use bendclaw::llm::circuit_breaker::CircuitBreaker;
use bendclaw::types::ErrorCode;

#[test]
fn test_starts_available() -> Result<()> {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    assert!(cb.is_available());
    assert_eq!(cb.failure_count(), 0);
    Ok(())
}

#[test]
fn test_stays_available_below_threshold() -> Result<()> {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.failure_count(), 2);
    assert!(cb.is_available());
    Ok(())
}

#[test]
fn test_trips_at_threshold() -> Result<()> {
    let cb = CircuitBreaker::new(3, Duration::from_secs(60));
    cb.record_failure();
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.failure_count(), 3);
    assert!(!cb.is_available());
    Ok(())
}

#[test]
fn test_success_resets() -> Result<()> {
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    cb.record_failure();
    cb.record_failure();
    assert!(!cb.is_available());
    cb.record_success();
    assert!(cb.is_available());
    assert_eq!(cb.failure_count(), 0);
    Ok(())
}

#[test]
fn test_half_open_after_cooldown() -> Result<()> {
    let cooldown = Duration::from_millis(100);
    let cb = CircuitBreaker::new(1, cooldown);
    cb.record_failure();
    assert!(!cb.is_available());
    std::thread::sleep(cooldown + Duration::from_millis(20));
    assert!(cb.is_available());
    Ok(())
}

#[test]
fn test_stays_open_before_cooldown_elapsed() -> Result<()> {
    let cb = CircuitBreaker::new(1, Duration::from_secs(1));
    cb.record_failure();
    assert!(!cb.is_available());
    Ok(())
}

#[test]
fn test_failure_count_continues_to_increment_after_tripped() -> Result<()> {
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    cb.record_failure();
    cb.record_failure();
    cb.record_failure();
    assert_eq!(cb.failure_count(), 3);
    assert!(!cb.is_available());
    Ok(())
}

// ── is_transient ──

#[test]
fn transient_rate_limit_error() {
    assert!(is_transient(&ErrorCode::llm_rate_limit(
        "rate limit exceeded"
    )));
}

#[test]
fn transient_server_error() {
    assert!(is_transient(&ErrorCode::llm_server(
        "internal server error"
    )));
}

#[test]
fn transient_timeout_error() {
    assert!(is_transient(&ErrorCode::timeout("request timed out")));
}

#[test]
fn transient_message_overloaded() {
    assert!(is_transient(&ErrorCode::llm_request("model is overloaded")));
}

#[test]
fn transient_message_503() {
    assert!(is_transient(&ErrorCode::llm_request("HTTP 503")));
}

#[test]
fn transient_message_connection() {
    assert!(is_transient(&ErrorCode::llm_request(
        "connection reset by peer"
    )));
}

#[test]
fn non_transient_auth_error() {
    assert!(!is_transient(&ErrorCode::llm_request("invalid API key")));
}

#[test]
fn non_transient_context_length() {
    assert!(!is_transient(&ErrorCode::llm_request(
        "context length exceeded"
    )));
}

// ── record_failure_if_transient ──

#[test]
fn record_failure_if_transient_counts_transient() -> Result<()> {
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    let e = ErrorCode::llm_rate_limit("rate limit");
    cb.record_failure_if_transient(&e);
    cb.record_failure_if_transient(&e);
    assert_eq!(cb.failure_count(), 2);
    assert!(!cb.is_available());
    Ok(())
}

#[test]
fn record_failure_if_transient_ignores_non_transient() -> Result<()> {
    let cb = CircuitBreaker::new(2, Duration::from_secs(60));
    let e = ErrorCode::llm_request("invalid API key");
    cb.record_failure_if_transient(&e);
    cb.record_failure_if_transient(&e);
    cb.record_failure_if_transient(&e);
    assert_eq!(cb.failure_count(), 0);
    assert!(cb.is_available());
    Ok(())
}
