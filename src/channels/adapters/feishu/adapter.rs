use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use super::config::FeishuConfig;
use super::config::ReconnectConfig;
use super::config::FEISHU_CHANNEL_TYPE;
use super::config::FEISHU_MAX_MESSAGE_LEN;
use super::outbound::FeishuOutbound;
use super::token::TokenCache;
use super::ws::ws_receive_loop;
use crate::channels::model::account::ChannelAccount;
use crate::channels::model::capabilities::ChannelCapabilities;
use crate::channels::model::capabilities::ChannelKind;
use crate::channels::model::capabilities::InboundMode;
use crate::channels::runtime::channel_trait::ChannelOutbound;
use crate::channels::runtime::channel_trait::ChannelPlugin;
use crate::channels::runtime::channel_trait::InboundEventSender;
use crate::channels::runtime::channel_trait::InboundKind;
use crate::channels::runtime::channel_trait::ReceiverFactory;
use crate::channels::runtime::diagnostics;
use crate::types::ErrorCode;
use crate::types::Result;

// ── Plugin ──

pub struct FeishuChannel {
    client: reqwest::Client,
    token_cache: Arc<TokenCache>,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            token_cache: Arc::new(TokenCache::new()),
        }
    }
}

impl Default for FeishuChannel {
    fn default() -> Self {
        Self::new()
    }
}
#[async_trait]
impl ChannelPlugin for FeishuChannel {
    fn channel_type(&self) -> &str {
        FEISHU_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::WebSocket,
            supports_edit: true,
            supports_streaming: false,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: FEISHU_MAX_MESSAGE_LEN,
            stale_event_threshold: Some(Duration::from_secs(600)),
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        let c: FeishuConfig = serde_json::from_value(config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        if c.app_id.is_empty() || c.app_secret.is_empty() {
            return Err(ErrorCode::config(
                "feishu app_id and app_secret are required",
            ));
        }
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FeishuOutbound {
            client: self.client.clone(),
            token_cache: self.token_cache.clone(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(FeishuReceiverFactory {
            client: self.client.clone(),
            token_cache: self.token_cache.clone(),
        }))
    }
}

// ── ReceiverFactory ──

struct FeishuReceiverFactory {
    client: reqwest::Client,
    token_cache: Arc<TokenCache>,
}

#[async_trait]
impl ReceiverFactory for FeishuReceiverFactory {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let config: FeishuConfig = serde_json::from_value(account.config.clone())
            .map_err(|e| ErrorCode::config(format!("invalid feishu config: {e}")))?;
        let client = self.client.clone();
        let token_cache = self.token_cache.clone();
        let account_id = account.channel_account_id.clone();

        let handle = crate::types::spawn_named("feishu_receiver", async move {
            diagnostics::log_feishu_receiver_started(&account_id);
            let mut reconnect_config = ReconnectConfig::default();
            let mut attempt: u64 = 0;

            const STABLE_CONNECTION_THRESHOLD: Duration = Duration::from_secs(60);

            loop {
                let connected_at = Instant::now();
                tokio::select! {
                    _ = cancel.cancelled() => {
                        diagnostics::log_feishu_receiver_cancelled(&account_id);
                        return;
                    }
                    result = ws_receive_loop(
                        &client, &config, &token_cache, &event_tx, &cancel,
                        &mut reconnect_config,
                    ) => {
                        event_tx.set_connected(false);
                        match result {
                            Ok(()) => {
                                diagnostics::log_feishu_closed_reconnecting(&account_id);
                                attempt = 0;
                            }
                            Err(e) => {
                                if e.code == ErrorCode::CONFIG {
                                    diagnostics::log_feishu_client_error_stopping(&account_id, &e);
                                    return;
                                }
                                if connected_at.elapsed() >= STABLE_CONNECTION_THRESHOLD {
                                    attempt = 0;
                                }
                                attempt += 1;
                                diagnostics::log_feishu_error_reconnecting(&account_id, attempt, &e);
                            }
                        }
                    }
                }

                if reconnect_config.reconnect_count > 0
                    && attempt >= reconnect_config.reconnect_count
                {
                    diagnostics::log_feishu_reconnect_limit_reached(
                        &account_id,
                        reconnect_config.reconnect_count,
                    );
                    return;
                }

                // Backoff: base interval * 2^attempt, capped at 120s
                let base = reconnect_config.reconnect_interval.max(1);
                let backoff = (base * 2u64.saturating_pow(attempt.min(6) as u32)).min(120);
                // Add nonce jitter on first reconnect
                let jitter = if attempt == 1 && reconnect_config.reconnect_nonce > 0 {
                    use std::collections::hash_map::DefaultHasher;
                    use std::hash::Hash;
                    use std::hash::Hasher;
                    let mut h = DefaultHasher::new();
                    account_id.hash(&mut h);
                    (h.finish() % reconnect_config.reconnect_nonce).min(30)
                } else {
                    0
                };
                let delay = Duration::from_secs(backoff + jitter);

                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(delay) => {}
                }
            }
        });

        Ok(handle)
    }
}
