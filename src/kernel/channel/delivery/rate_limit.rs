use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

/// Configuration for a token bucket rate limiter.
pub struct RateLimitConfig {
    /// Maximum burst size (bucket capacity).
    pub burst: u32,
    /// Token refill rate (tokens per second).
    pub refill_rate: f64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            burst: 10,
            refill_rate: 5.0,
        }
    }
}

/// Result of a rate limit check.
pub enum RateLimitResult {
    Allowed,
    RetryAfter(Duration),
}

struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
    last_used: Instant,
    config: RateLimitConfig,
}

impl TokenBucket {
    fn new(config: RateLimitConfig) -> Self {
        let now = Instant::now();
        Self {
            tokens: config.burst as f64,
            last_refill: now,
            last_used: now,
            config,
        }
    }

    fn try_acquire(&mut self) -> RateLimitResult {
        self.refill();
        self.last_used = Instant::now();
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            RateLimitResult::Allowed
        } else {
            let deficit = 1.0 - self.tokens;
            let wait_secs = deficit / self.config.refill_rate;
            RateLimitResult::RetryAfter(Duration::from_secs_f64(wait_secs))
        }
    }

    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        let added = elapsed * self.config.refill_rate;
        self.tokens = (self.tokens + added).min(self.config.burst as f64);
        self.last_refill = now;
    }
}

/// Per-(channel_type, account_id) outbound rate limiter.
pub struct OutboundRateLimiter {
    buckets: parking_lot::Mutex<HashMap<String, TokenBucket>>,
    default_config: RateLimitConfig,
}

impl OutboundRateLimiter {
    pub fn new(default_config: RateLimitConfig) -> Self {
        Self {
            buckets: parking_lot::Mutex::new(HashMap::new()),
            default_config,
        }
    }

    /// Check rate limit for a given channel+account pair.
    pub fn check(&self, channel_type: &str, account_id: &str) -> RateLimitResult {
        let key = format!("{channel_type}:{account_id}");
        let mut buckets = self.buckets.lock();
        let bucket = buckets.entry(key).or_insert_with(|| {
            TokenBucket::new(RateLimitConfig {
                burst: self.default_config.burst,
                refill_rate: self.default_config.refill_rate,
            })
        });
        bucket.try_acquire()
    }

    /// Async helper: loops until a token is acquired.
    pub async fn wait_if_needed(&self, channel_type: &str, account_id: &str) {
        loop {
            match self.check(channel_type, account_id) {
                RateLimitResult::Allowed => return,
                RateLimitResult::RetryAfter(dur) => {
                    tracing::debug!(
                        channel_type,
                        account_id,
                        wait_ms = dur.as_millis() as u64,
                        "rate limiter: waiting"
                    );
                    tokio::time::sleep(dur).await;
                }
            }
        }
    }

    /// Remove buckets that haven't been used for longer than `max_idle`.
    pub fn evict_stale(&self, max_idle: Duration) {
        let now = Instant::now();
        self.buckets
            .lock()
            .retain(|_, bucket| now.duration_since(bucket.last_used) < max_idle);
    }
}
