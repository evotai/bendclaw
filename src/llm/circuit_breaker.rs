use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

use parking_lot::Mutex;

/// Tracks consecutive failures and trips open after a threshold.
///
/// States:
/// - Closed (healthy): `consecutive_failures < threshold`
/// - Open (tripped): failures reached threshold, requests blocked
/// - Half-open: cooldown elapsed, one probe request allowed
///
/// ```ignore
/// let breaker = CircuitBreaker::new(3, Duration::from_secs(60));
/// if breaker.is_available() {
///     match do_request().await {
///         Ok(_) => breaker.record_success(),
///         Err(_) => breaker.record_failure(),
///     }
/// }
/// ```
pub struct CircuitBreaker {
    consecutive_failures: AtomicU32,
    tripped_at: Mutex<Option<Instant>>,
    threshold: u32,
    cooldown: Duration,
}

impl CircuitBreaker {
    pub fn new(threshold: u32, cooldown: Duration) -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            tripped_at: Mutex::new(None),
            threshold,
            cooldown,
        }
    }

    /// Returns `true` if the circuit is closed or half-open (cooldown expired).
    pub fn is_available(&self) -> bool {
        let failures = self.consecutive_failures.load(Ordering::Relaxed);
        if failures == 0 {
            return true;
        }
        let guard = self.tripped_at.lock();
        match *guard {
            None => true,
            Some(t) => t.elapsed() >= self.cooldown,
        }
    }

    /// Reset on success — close the circuit.
    pub fn record_success(&self) {
        let prev = self.consecutive_failures.load(Ordering::Relaxed);
        self.consecutive_failures.store(0, Ordering::Relaxed);
        let mut guard = self.tripped_at.lock();
        if guard.is_some() {
            tracing::info!(previous_failures = prev, "circuit breaker recovered");
        }
        *guard = None;
    }

    /// Increment failures. Trips the circuit when threshold is reached.
    pub fn record_failure(&self) {
        let prev = self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        if prev + 1 >= self.threshold {
            let mut guard = self.tripped_at.lock();
            if guard.is_none() {
                tracing::warn!(
                    failures = prev + 1,
                    threshold = self.threshold,
                    cooldown_secs = self.cooldown.as_secs(),
                    "circuit breaker tripped"
                );
            }
            *guard = Some(Instant::now());
        }
    }

    pub fn failure_count(&self) -> u32 {
        self.consecutive_failures.load(Ordering::Relaxed)
    }
}
