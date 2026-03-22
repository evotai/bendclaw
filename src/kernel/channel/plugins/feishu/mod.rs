//! Feishu/Lark channel plugin.
//!
//! Submodules:
//! - `config`   — FeishuConfig, allow_from, mention_only
//! - `token`    — tenant access token cache + fetch
//! - `message`  — text/post parsing, @mention cleanup, dedup
//! - `outbound` — send_text, edit_message (with token retry)
//! - `ws`       — WebSocket long-connection receive loop

pub mod config;
pub mod message;
pub mod outbound;
pub mod token;
pub mod ws;

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::capabilities::{ChannelCapabilities, ChannelKind, InboundMode};
use crate::kernel::channel::plugin::{
    ChannelOutbound, ChannelPlugin, InboundEventSender, InboundKind, ReceiverFactory,
};
use crate::observability::log::slog;

use config::FeishuConfig;
use message::MessageDedup;
use outbound::FeishuOutbound;
use token::TokenCache;

pub const FEISHU_CHANNEL_TYPE: &str = "feishu";
const FEISHU_MAX_MESSAGE_LEN: usize = 30_000;

// ── Plugin ────────────────────────────────────────────────────────────────────

pub struct FeishuChannel {
    client: reqwest::Client,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self { client: reqwest::Client::new() }
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
        }
    }

    fn validate_config(&self, config: &serde_json::Value) -> Result<()> {
        FeishuConfig::from_json(config)?.validate()
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(FeishuOutbound {
            client: self.client.clone(),
            token_cache: TokenCache::new(),
        })
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::Receiver(Arc::new(FeishuReceiverFactory {
            client: self.client.clone(),
        }))
    }
}

// ── ReceiverFactory ───────────────────────────────────────────────────────────

struct FeishuReceiverFactory {
    client: reqwest::Client,
}

#[async_trait]
impl ReceiverFactory for FeishuReceiverFactory {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let config = FeishuConfig::from_json(&account.config)?;
        let client = self.client.clone();
        let account_id = account.channel_account_id.clone();
        let token_cache = TokenCache::new();
        let dedup = Arc::new(RwLock::new(MessageDedup::new()));

        let handle = tokio::spawn(async move {
            slog!(info, "feishu_ws", "receiver_started", account_id = %account_id,);
            let mut backoff_secs = ws::RECONNECT_DELAY_SECS;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        slog!(info, "feishu_ws", "receiver_cancelled", account_id = %account_id,);
                        return;
                    }
                    result = ws::receive_loop(
                        &client, &config, &event_tx, &cancel,
                        &token_cache, &dedup, None,
                    ) => {
                        match result {
                            Ok(()) => {
                                slog!(info, "feishu_ws", "closed_reconnecting", account_id = %account_id,);
                                backoff_secs = ws::RECONNECT_DELAY_SECS;
                            }
                            Err(e) => {
                                slog!(error, "feishu_ws", "error_reconnecting",
                                    account_id = %account_id, backoff_secs, error = %e,);
                            }
                        }
                    }
                }

                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)) => {}
                }
                backoff_secs = (backoff_secs * 2).min(ws::MAX_BACKOFF_SECS);
            }
        });

        Ok(handle)
    }
}
