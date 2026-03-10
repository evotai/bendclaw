use std::time::Duration;

use anyhow::Result;
use bendclaw::llm::circuit_breaker::CircuitBreaker;

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
