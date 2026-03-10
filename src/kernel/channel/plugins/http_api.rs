use std::sync::Arc;

use async_trait::async_trait;

use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::channel::capabilities::ChannelCapabilities;
use crate::kernel::channel::capabilities::ChannelKind;
use crate::kernel::channel::capabilities::InboundMode;
use crate::kernel::channel::plugin::ChannelOutbound;
use crate::kernel::channel::plugin::ChannelPlugin;
use crate::kernel::channel::plugin::InboundKind;

pub const HTTP_API_CHANNEL_TYPE: &str = "http_api";

pub struct HttpApiChannel;

impl HttpApiChannel {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpApiChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ChannelPlugin for HttpApiChannel {
    fn channel_type(&self) -> &str {
        HTTP_API_CHANNEL_TYPE
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            channel_kind: ChannelKind::Conversational,
            inbound_mode: InboundMode::HttpRequest,
            supports_edit: false,
            supports_streaming: true,
            supports_markdown: true,
            supports_threads: false,
            supports_reactions: false,
            max_message_len: 1_000_000,
        }
    }

    fn validate_config(&self, _config: &serde_json::Value) -> Result<()> {
        Ok(())
    }

    fn outbound(&self) -> Arc<dyn ChannelOutbound> {
        Arc::new(HttpApiOutbound)
    }

    fn inbound(&self) -> InboundKind {
        InboundKind::None
    }
}

struct HttpApiOutbound;

#[async_trait]
impl ChannelOutbound for HttpApiOutbound {
    async fn send_text(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _text: &str,
    ) -> Result<String> {
        Err(ErrorCode::internal(
            "http_api channel uses SSE streaming, not outbound send_text",
        ))
    }

    async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> Result<()> {
        Ok(())
    }

    async fn edit_message(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _text: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "http_api channel does not support edit_message",
        ))
    }

    async fn add_reaction(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        _msg_id: &str,
        _emoji: &str,
    ) -> Result<()> {
        Err(ErrorCode::internal(
            "http_api channel does not support reactions",
        ))
    }
}
