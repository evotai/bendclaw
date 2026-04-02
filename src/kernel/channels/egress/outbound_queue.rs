use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::kernel::channels::egress::retry::send_with_retry;
use crate::kernel::channels::egress::retry::RetryConfig;
use crate::kernel::channels::runtime::channel_trait::ChannelOutbound;
use crate::kernel::channels::runtime::diagnostics;

/// A message queued for delayed retry after immediate retries failed.
pub struct QueuedMessage {
    pub outbound: Arc<dyn ChannelOutbound>,
    pub config: serde_json::Value,
    pub chat_id: String,
    pub text: String,
    pub attempt: usize,
    pub next_attempt_at: Instant,
}

pub struct OutboundQueueConfig {
    pub capacity: usize,
    pub max_attempts: usize,
    pub retry_config: RetryConfig,
}

impl Default for OutboundQueueConfig {
    fn default() -> Self {
        Self {
            capacity: 256,
            max_attempts: 5,
            retry_config: RetryConfig::default(),
        }
    }
}

/// Handle to the outbound retry queue.
pub struct OutboundQueue {
    tx: mpsc::Sender<QueuedMessage>,
}

impl OutboundQueue {
    /// No-op queue that silently drops all messages. For tests.
    pub fn noop() -> Self {
        let (tx, _rx) = mpsc::channel(1);
        Self { tx }
    }

    /// Enqueue a message for delayed retry. Non-blocking; drops on full.
    pub fn enqueue(&self, msg: QueuedMessage) {
        if self.tx.try_send(msg).is_err() {
            diagnostics::log_channel_queue_full();
        }
    }
}

/// Spawn the outbound retry queue. Returns a handle for enqueuing messages.
pub fn spawn_outbound_queue(config: OutboundQueueConfig) -> OutboundQueue {
    let (tx, rx) = mpsc::channel(config.capacity);
    let re_enqueue_tx = tx.clone();
    let max_attempts = config.max_attempts;
    let retry_config = Arc::new(config.retry_config);

    crate::types::spawn_fire_and_forget(
        "outbound_dispatch_loop",
        dispatch_loop(rx, re_enqueue_tx, max_attempts, retry_config),
    );

    OutboundQueue { tx }
}

/// Receives messages and spawns a task per message — no head-of-line blocking.
async fn dispatch_loop(
    mut rx: mpsc::Receiver<QueuedMessage>,
    re_enqueue_tx: mpsc::Sender<QueuedMessage>,
    max_attempts: usize,
    retry_config: Arc<RetryConfig>,
) {
    while let Some(msg) = rx.recv().await {
        let tx = re_enqueue_tx.clone();
        let cfg = retry_config.clone();
        crate::types::spawn_fire_and_forget("outbound_message_handler", async move {
            handle_queued_message(msg, max_attempts, &cfg, &tx).await;
        });
    }
}

async fn handle_queued_message(
    msg: QueuedMessage,
    max_attempts: usize,
    retry_cfg: &RetryConfig,
    re_enqueue_tx: &mpsc::Sender<QueuedMessage>,
) {
    // Wait until the scheduled retry time.
    let now = Instant::now();
    if msg.next_attempt_at > now {
        tokio::time::sleep(msg.next_attempt_at - now).await;
    }

    let ob = msg.outbound.clone();
    let config = msg.config.clone();
    let chat_id = msg.chat_id.clone();
    let text = msg.text.clone();

    let result = send_with_retry(
        || {
            let ob = ob.clone();
            let config = config.clone();
            let chat_id = chat_id.clone();
            let text = text.clone();
            async move { ob.send_text(&config, &chat_id, &text).await }
        },
        retry_cfg,
    )
    .await;

    match result {
        Ok(_) => {}
        Err(e) if msg.attempt < max_attempts => {
            let backoff = Duration::from_secs(2u64.pow(msg.attempt as u32).min(60));
            diagnostics::log_channel_retry(&e, msg.attempt, backoff.as_secs());
            let next = QueuedMessage {
                outbound: msg.outbound,
                config: msg.config,
                chat_id: msg.chat_id,
                text: msg.text,
                attempt: msg.attempt + 1,
                next_attempt_at: Instant::now() + backoff,
            };
            if re_enqueue_tx.try_send(next).is_err() {
                diagnostics::log_channel_dead_letter();
            }
        }
        Err(e) => {
            diagnostics::log_channel_dead_letter_failed(&e, msg.attempt, &msg.chat_id);
        }
    }
}
