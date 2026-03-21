use std::time::Duration;

use bendclaw::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use bendclaw::kernel::channel::delivery::rate_limit::RateLimitConfig;
use bendclaw::kernel::channel::delivery::rate_limit::RateLimitResult;

#[test]
fn allows_up_to_burst() {
    let limiter = OutboundRateLimiter::new(RateLimitConfig {
        burst: 3,
        refill_rate: 1.0,
    });
    for _ in 0..3 {
        assert!(matches!(limiter.check("feishu", "acc1"), RateLimitResult::Allowed));
    }
    assert!(matches!(limiter.check("feishu", "acc1"), RateLimitResult::RetryAfter(_)));
}

#[test]
fn separate_buckets_per_key() {
    let limiter = OutboundRateLimiter::new(RateLimitConfig {
        burst: 1,
        refill_rate: 1.0,
    });
    assert!(matches!(limiter.check("feishu", "acc1"), RateLimitResult::Allowed));
    assert!(matches!(limiter.check("feishu", "acc2"), RateLimitResult::Allowed));
    // acc1 exhausted
    assert!(matches!(limiter.check("feishu", "acc1"), RateLimitResult::RetryAfter(_)));
    // acc2 exhausted
    assert!(matches!(limiter.check("feishu", "acc2"), RateLimitResult::RetryAfter(_)));
}

#[test]
fn retry_after_duration_is_positive() {
    let limiter = OutboundRateLimiter::new(RateLimitConfig {
        burst: 1,
        refill_rate: 10.0,
    });
    assert!(matches!(limiter.check("tg", "a"), RateLimitResult::Allowed));
    match limiter.check("tg", "a") {
        RateLimitResult::RetryAfter(d) => {
            assert!(d > Duration::ZERO);
            assert!(d < Duration::from_secs(1));
        }
        RateLimitResult::Allowed => panic!("expected RetryAfter"),
    }
}

#[test]
fn evict_stale_removes_old_buckets() {
    let limiter = OutboundRateLimiter::new(RateLimitConfig {
        burst: 5,
        refill_rate: 1.0,
    });
    limiter.check("feishu", "old");
    // Evict with zero idle tolerance — everything is stale.
    limiter.evict_stale(Duration::ZERO);
    // Bucket was evicted, so a new one is created with full burst.
    for _ in 0..5 {
        assert!(matches!(limiter.check("feishu", "old"), RateLimitResult::Allowed));
    }
}

#[tokio::test]
async fn wait_if_needed_reacquires_token() {
    let limiter = std::sync::Arc::new(OutboundRateLimiter::new(RateLimitConfig {
        burst: 1,
        refill_rate: 100.0,
    }));

    // Exhaust the single token.
    assert!(matches!(limiter.check("ch", "a"), RateLimitResult::Allowed));
    assert!(matches!(limiter.check("ch", "a"), RateLimitResult::RetryAfter(_)));

    // wait_if_needed should loop until a token is available.
    let start = std::time::Instant::now();
    limiter.wait_if_needed("ch", "a").await;
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(1));

    // After wait_if_needed, the token was consumed — next check should fail.
    assert!(matches!(limiter.check("ch", "a"), RateLimitResult::RetryAfter(_)));
}

#[tokio::test]
async fn concurrent_senders_respect_burst() {
    let limiter = std::sync::Arc::new(OutboundRateLimiter::new(RateLimitConfig {
        burst: 2,
        refill_rate: 0.5, // slow refill — 1 token per 2 seconds
    }));

    // Exhaust both tokens.
    limiter.check("ch", "a");
    limiter.check("ch", "a");

    // Spawn 3 concurrent waiters.
    let mut handles = Vec::new();
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    for _ in 0..3 {
        let l = limiter.clone();
        let c = counter.clone();
        handles.push(tokio::spawn(async move {
            l.wait_if_needed("ch", "a").await;
            c.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));
    }

    // After 100ms, at most 0 should have completed (refill is 0.5/sec).
    tokio::time::sleep(Duration::from_millis(100)).await;
    let completed = counter.load(std::sync::atomic::Ordering::SeqCst);
    assert!(completed <= 1, "at most 1 should complete quickly, got {completed}");
}
