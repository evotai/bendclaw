use std::sync::Arc;

use async_trait::async_trait;
use axum::http::HeaderMap;
use tokio_util::sync::CancellationToken;

use crate::base::Result;

use super::account::ChannelAccount;
use super::capabilities::ChannelCapabilities;
use super::message::InboundEvent;

/// Sender for pushing inbound events from long-lived connections (WebSocket, polling).
pub type InboundEventSender = tokio::sync::mpsc::UnboundedSender<InboundEvent>;

/// Describes how a channel receives inbound events.
pub enum InboundKind {
    /// Channel uses webhook delivery (e.g. GitHub).
    Webhook(Arc<dyn WebhookHandler>),
    /// Channel uses a long-lived connection (WebSocket / polling).
    Receiver(Arc<dyn ReceiverFactory>),
    /// Channel has no inbound (e.g. http_api — inbound is the runs endpoint).
    None,
}

/// Channel plugin. Each transport (HTTP, Telegram, Feishu, GitHub) implements this once.
#[async_trait]
pub trait ChannelPlugin: Send + Sync {
    /// Unique identifier for this channel type (e.g. "http_api", "telegram", "feishu").
    fn channel_type(&self) -> &str;

    /// Declare what this channel supports.
    fn capabilities(&self) -> ChannelCapabilities;

    /// Validate channel-specific config JSON.
    fn validate_config(&self, config: &serde_json::Value) -> Result<()>;

    /// Outbound message interface.
    fn outbound(&self) -> Arc<dyn ChannelOutbound>;

    /// Describe how this channel receives inbound events.
    fn inbound(&self) -> InboundKind;
}

/// Factory that spawns a background receiver task for a single channel account.
#[async_trait]
pub trait ReceiverFactory: Send + Sync {
    async fn spawn(
        &self,
        account: &ChannelAccount,
        event_tx: InboundEventSender,
        cancel: CancellationToken,
    ) -> Result<tokio::task::JoinHandle<()>>;
}

/// Outbound message interface. Separated from ChannelPlugin for easy mocking.
#[async_trait]
pub trait ChannelOutbound: Send + Sync {
    /// Send text, return platform message ID.
    async fn send_text(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        text: &str,
    ) -> Result<String>;

    async fn send_typing(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
    ) -> Result<()>;

    async fn edit_message(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        text: &str,
    ) -> Result<()>;

    async fn add_reaction(
        &self,
        config: &serde_json::Value,
        chat_id: &str,
        msg_id: &str,
        emoji: &str,
    ) -> Result<()>;
}

/// Webhook verification + parsing for webhook-mode channels.
pub trait WebhookHandler: Send + Sync {
    fn verify(&self, external_account_id: &str, headers: &HeaderMap, body: &[u8]) -> Result<()>;
    fn parse(&self, external_account_id: &str, body: &[u8]) -> Result<Vec<InboundEvent>>;

    /// Return a challenge response for webhook verification handshakes (e.g. Feishu url_verification).
    fn challenge_response(&self, _body: &[u8]) -> Option<serde_json::Value> {
        None
    }
}
