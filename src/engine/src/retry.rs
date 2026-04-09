//! Retry with exponential backoff and jitter for provider calls.

use std::time::Duration;

use crate::provider::ProviderError;

/// Configuration for automatic retry of transient provider errors.
///
/// Defaults: 10 retries, 1s initial delay, 2x backoff, 30s max delay.
/// Use `RetryConfig::none()` to disable retries entirely.
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (0 = no retries).
    pub max_retries: usize,
    /// Initial delay before the first retry (milliseconds).
    pub initial_delay_ms: u64,
    /// Multiplier applied to the delay after each attempt.
    pub backoff_multiplier: f64,
    /// Maximum delay between retries (milliseconds).
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 10,
            initial_delay_ms: 1000,
            backoff_multiplier: 2.0,
            max_delay_ms: 30_000,
        }
    }
}

impl RetryConfig {
    /// No retries — fail immediately on any error.
    pub fn none() -> Self {
        Self {
            max_retries: 0,
            ..Default::default()
        }
    }

    /// Calculate the delay for a given attempt (1-indexed).
    /// Uses exponential backoff with ±20% jitter.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let base_ms =
            self.initial_delay_ms as f64 * self.backoff_multiplier.powi((attempt - 1) as i32);
        let capped_ms = base_ms.min(self.max_delay_ms as f64);

        // Jitter: ±20% (multiply by 0.8–1.2)
        let jitter = 0.8 + rand::random::<f64>() * 0.4;
        Duration::from_millis((capped_ms * jitter) as u64)
    }
}

impl ProviderError {
    /// Whether this error is safe to retry.
    ///
    /// Retryable: rate limits (429), network/transient errors, API errors (400),
    /// and auth errors (401/403) — these can be transient.
    /// Not retryable: context overflow, cancellation.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Network(_) | Self::Api(_) | Self::Auth(_)
        )
    }

    /// If this is a rate limit with a server-specified retry delay, return it.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited {
                retry_after_ms: Some(ms),
            } => Some(Duration::from_millis(*ms)),
            _ => None,
        }
    }
}
