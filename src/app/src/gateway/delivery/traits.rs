use async_trait::async_trait;

use crate::error::Result;

#[derive(Debug, Clone, Copy)]
pub struct DeliveryCapabilities {
    pub can_edit: bool,
    pub max_message_len: usize,
}

#[async_trait]
pub trait MessageSink: Send + Sync {
    fn capabilities(&self) -> DeliveryCapabilities;

    /// Send text message, return platform message_id.
    async fn send_text(&self, chat_id: &str, text: &str) -> Result<String>;

    /// Edit an existing message in-place. No-op if unsupported.
    async fn edit_text(&self, chat_id: &str, message_id: &str, text: &str) -> Result<()> {
        let _ = (chat_id, message_id, text);
        Ok(())
    }
}
