//! Retry policy for transient provider errors.
//!
//! Defines [`RetryPolicy`] (backoff timing) and [`should_retry()`] (error
//! classification). The agent loop combines both to decide whether and
//! when to re-attempt a failed provider call.

use std::time::Duration;

use crate::provider::ProviderError;

/// Retry policy with exponential backoff.
///
/// Controls *how many* times and *how long* to wait between retries.
/// Use [`RetryPolicy::disabled()`] to fail immediately on any error.
///
/// Internal backoff parameters (1 s initial, 2× multiplier, 30 s cap,
/// ±20 % jitter) are intentionally not exposed — callers express intent
/// via [`new()`](RetryPolicy::new) and the
/// implementation is free to evolve.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    max_retries: usize,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_retries: 10 }
    }
}

// Internal backoff constants.
const INITIAL_DELAY_MS: f64 = 1000.0;
const BACKOFF_MULTIPLIER: f64 = 2.0;
const MAX_DELAY_MS: f64 = 30_000.0;

impl RetryPolicy {
    /// No retries — fail immediately on any error.
    pub fn disabled() -> Self {
        Self { max_retries: 0 }
    }

    /// Create a policy that retries up to `n` times.
    pub fn new(n: usize) -> Self {
        Self { max_retries: n }
    }

    /// Maximum number of retry attempts (0 = no retries).
    pub fn max_retries(&self) -> usize {
        self.max_retries
    }

    /// Calculate the delay for a given attempt (1-indexed).
    /// Uses exponential backoff with ±20 % jitter.
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        let base_ms = INITIAL_DELAY_MS * BACKOFF_MULTIPLIER.powi((attempt - 1) as i32);
        let capped_ms = base_ms.min(MAX_DELAY_MS);

        // Jitter: ±20 % (multiply by 0.8–1.2)
        let jitter = 0.8 + rand::random::<f64>() * 0.4;
        Duration::from_millis((capped_ms * jitter) as u64)
    }
}

/// Whether this provider error is safe to retry.
///
/// Retryable: rate limits (429), network/transient errors, API errors
/// (5xx, 529 overloaded).
/// Not retryable: auth (401/403), context overflow, cancellation,
/// client errors (400 etc.).
pub fn should_retry(error: &ProviderError) -> bool {
    matches!(
        error,
        ProviderError::RateLimited { .. } | ProviderError::Network(_) | ProviderError::Api(_)
    )
}
