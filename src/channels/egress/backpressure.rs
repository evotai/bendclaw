use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::model::message::InboundEvent;
use crate::channels::model::status::ChannelStatus;
use crate::channels::runtime::diagnostics;

pub struct BackpressureConfig {
    pub busy_threshold: usize,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self { busy_threshold: 50 }
    }
}

pub enum BackpressureResult {
    Accepted,
    Busy,
    Rejected,
}

pub struct BackpressureSender {
    inner: mpsc::Sender<InboundEvent>,
    busy_threshold: usize,
    status: Arc<ChannelStatus>,
    account_id: String,
}

impl BackpressureSender {
    pub fn new(
        inner: mpsc::Sender<InboundEvent>,
        config: BackpressureConfig,
        status: Arc<ChannelStatus>,
        account_id: String,
    ) -> Self {
        Self {
            inner,
            busy_threshold: config.busy_threshold,
            status,
            account_id,
        }
    }

    pub fn send(&self, event: InboundEvent) -> BackpressureResult {
        let remaining = self.inner.capacity();

        if remaining == 0 {
            match self.inner.try_send(event) {
                Ok(()) => {
                    self.touch();
                    BackpressureResult::Busy
                }
                Err(_) => {
                    diagnostics::log_channel_rejected();
                    BackpressureResult::Rejected
                }
            }
        } else if remaining <= self.busy_threshold {
            match self.inner.try_send(event) {
                Ok(()) => {
                    self.touch();
                    diagnostics::log_channel_busy(remaining, self.busy_threshold);
                    BackpressureResult::Busy
                }
                Err(_) => {
                    diagnostics::log_channel_rejected();
                    BackpressureResult::Rejected
                }
            }
        } else {
            match self.inner.try_send(event) {
                Ok(()) => {
                    self.touch();
                    BackpressureResult::Accepted
                }
                Err(_) => BackpressureResult::Rejected,
            }
        }
    }

    pub fn remaining_capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn set_connected(&self, connected: bool) {
        self.status.set_connected(&self.account_id, connected);
    }

    fn touch(&self) {
        self.status.touch_event(&self.account_id);
    }
}
