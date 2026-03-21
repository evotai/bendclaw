use tokio::sync::mpsc;

use crate::kernel::channel::message::InboundEvent;

pub struct BackpressureConfig {
    /// When remaining capacity drops below this, reply "busy".
    pub busy_threshold: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            busy_threshold: 50,
        }
    }
}

/// Result of attempting to send through the backpressure layer.
pub enum BackpressureResult {
    /// Message accepted into the queue.
    Accepted,
    /// Queue is nearly full — a "busy" reply should be sent.
    Busy,
    /// Queue is completely full — message was dropped.
    Rejected,
}

/// Wraps an mpsc::Sender with capacity-aware backpressure.
pub struct BackpressureSender {
    inner: mpsc::Sender<InboundEvent>,
    busy_threshold: usize,
}

impl BackpressureSender {
    pub fn new(inner: mpsc::Sender<InboundEvent>, config: BackpressureConfig) -> Self {
        Self {
            inner,
            busy_threshold: config.busy_threshold,
        }
    }

    /// Try to send an event with backpressure awareness.
    pub fn send(&self, event: InboundEvent) -> BackpressureResult {
        let remaining = self.inner.capacity();

        if remaining == 0 {
            match self.inner.try_send(event) {
                Ok(()) => BackpressureResult::Busy,
                Err(_) => {
                    tracing::warn!("backpressure: queue full, message rejected");
                    BackpressureResult::Rejected
                }
            }
        } else if remaining <= self.busy_threshold {
            match self.inner.try_send(event) {
                Ok(()) => {
                    tracing::debug!(
                        remaining,
                        threshold = self.busy_threshold,
                        "backpressure: queue nearly full"
                    );
                    BackpressureResult::Busy
                }
                Err(_) => {
                    tracing::warn!("backpressure: queue full, message rejected");
                    BackpressureResult::Rejected
                }
            }
        } else {
            match self.inner.try_send(event) {
                Ok(()) => BackpressureResult::Accepted,
                Err(_) => BackpressureResult::Rejected,
            }
        }
    }

    /// Returns the underlying sender's available capacity.
    pub fn remaining_capacity(&self) -> usize {
        self.inner.capacity()
    }
}
