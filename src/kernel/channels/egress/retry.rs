use std::future::Future;
use std::time::Duration;

use backon::ExponentialBuilder;
use backon::Retryable;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::channels::runtime::diagnostics;

/// Retry configuration for channel delivery operations.
pub struct RetryConfig {
    pub max_retries: usize,
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            min_delay_ms: 500,
            max_delay_ms: 5000,
        }
    }
}

/// Classify whether a channel error is transient and worth retrying.
pub fn is_channel_retryable(e: &ErrorCode) -> bool {
    // Never retry client errors (4xx) — they indicate a permanent problem.
    if is_client_error(&e.message) {
        return false;
    }
    matches!(
        e.code,
        ErrorCode::TIMEOUT
            | ErrorCode::INTERNAL
            | ErrorCode::CHANNEL_SEND
            | ErrorCode::CHANNEL_TIMEOUT
            | ErrorCode::CHANNEL_RATE_LIMITED
    ) || is_transient_message(&e.message)
}

fn is_client_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("http 400")
        || m.contains("http 401")
        || m.contains("http 403")
        || m.contains("http 404")
        || m.contains("http 405")
        || m.contains("http 422")
}

fn is_transient_message(msg: &str) -> bool {
    let m = msg.to_lowercase();
    m.contains("timeout")
        || m.contains("connection")
        || m.contains("reset by peer")
        || m.contains("http 5")
        || m.contains("rate limit")
        || m.contains("too many")
}

fn backoff_builder(config: &RetryConfig) -> ExponentialBuilder {
    ExponentialBuilder::default()
        .with_min_delay(Duration::from_millis(config.min_delay_ms))
        .with_max_delay(Duration::from_millis(config.max_delay_ms))
        .with_max_times(config.max_retries)
}

/// Generic retry wrapper for any async operation returning Result<T>.
pub async fn send_with_retry<F, Fut, T>(mut op: F, config: &RetryConfig) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    (|| op())
        .retry(backoff_builder(config))
        .when(|e: &ErrorCode| is_channel_retryable(e))
        .notify(|e: &ErrorCode, dur: Duration| {
            diagnostics::log_channel_retry_after(e, dur.as_millis() as u64);
        })
        .await
}
