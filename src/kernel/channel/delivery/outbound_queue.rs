use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::kernel::channel::delivery::retry::send_with_retry;
use crate::kernel::channel::delivery::retry::RetryConfig;
use crate::kernel::channel::plugin::ChannelOutbound;

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
            tracing::warn!("outbound_queue: queue full, dropping message");
        }
    }
}

/// Spawn the outbound retry queue. Returns a handle for enqueuing messages.
pub fn spawn_outbound_queue(config: OutboundQueueConfig) -> OutboundQueue {
    let (tx, rx) = mpsc::channel(config.capacity);
    let re_enqueue_tx = tx.clone();
    let max_attempts = config.max_attempts;
    let retry_config = Arc::new(config.retry_config);

    tokio::spawn(dispatch_loop(rx, re_enqueue_tx, max_attempts, retry_config));

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
        tokio::spawn(async move {
            handle_queued_message(msg, max_attempts, &cfg, &tx).await;
        });
    }
    tracing::info!("outbound_queue: channel closed, stopping");
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
        Ok(msg_id) => {
            tracing::info!(
                message_id = %msg_id,
                attempt = msg.attempt,
                "outbound_queue: delivered on retry"
            );
        }
        Err(e) if msg.attempt < max_attempts => {
            let backoff = Duration::from_secs(2u64.pow(msg.attempt as u32).min(60));
            tracing::warn!(
                error = %e,
                attempt = msg.attempt,
                next_backoff_secs = backoff.as_secs(),
                "outbound_queue: retry failed, re-enqueuing"
            );
            let next = QueuedMessage {
                outbound: msg.outbound,
                config: msg.config,
                chat_id: msg.chat_id,
                text: msg.text,
                attempt: msg.attempt + 1,
                next_attempt_at: Instant::now() + backoff,
            };
            if re_enqueue_tx.try_send(next).is_err() {
                tracing::error!("outbound_queue: re-enqueue failed, queue full — dead letter");
            }
        }
        Err(e) => {
            tracing::error!(
                error = %e,
                attempt = msg.attempt,
                chat_id = %msg.chat_id,
                "outbound_queue: dead letter — max attempts exceeded"
            );
        }
    }
}
