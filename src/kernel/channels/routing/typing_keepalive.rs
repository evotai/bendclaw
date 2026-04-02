use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use crate::kernel::channels::runtime::channel_trait::ChannelOutbound;

pub struct TypingKeepaliveConfig {
    pub interval: Duration,
    pub ttl: Duration,
}

impl Default for TypingKeepaliveConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(5),
            ttl: Duration::from_secs(120),
        }
    }
}

/// Periodically refreshes the typing indicator for the full dispatch lifecycle.
/// Spawns a background task that calls `send_typing` at a fixed interval
/// until stopped or TTL expires.
pub struct TypingKeepalive {
    cancel: CancellationToken,
    handle: JoinHandle<()>,
}

impl TypingKeepalive {
    pub fn start(
        outbound: Arc<dyn ChannelOutbound>,
        channel_config: serde_json::Value,
        chat_id: String,
        config: TypingKeepaliveConfig,
    ) -> Self {
        let cancel = CancellationToken::new();
        let token = cancel.clone();

        let handle = tokio::spawn(async move {
            // Send typing immediately so short responses still show the indicator.
            let _ = outbound.send_typing(&channel_config, &chat_id).await;

            let deadline = Instant::now() + config.ttl;
            loop {
                tokio::select! {
                    _ = token.cancelled() => break,
                    _ = tokio::time::sleep(config.interval) => {
                        if Instant::now() >= deadline {
                            break;
                        }
                        let _ = outbound.send_typing(&channel_config, &chat_id).await;
                    }
                }
            }
        });

        Self { cancel, handle }
    }

    pub fn stop(self) {
        self.cancel.cancel();
        self.handle.abort();
    }
}
