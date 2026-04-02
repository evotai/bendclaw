use serde::Deserialize;
use serde::Serialize;

/// Unified inbound event covering conversational, event-driven, and interactive patterns.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum InboundEvent {
    /// Conversational message (Telegram, Feishu, Slack DM, HTTP API).
    Message(InboundMessage),

    /// Platform event (GitHub PR opened, Slack channel_join).
    PlatformEvent {
        event_type: String,
        payload: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_context: Option<ReplyContext>,
    },

    /// Interactive callback (Slack button click, Telegram inline keyboard).
    Callback {
        callback_id: String,
        data: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reply_context: Option<ReplyContext>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InboundMessage {
    pub message_id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub sender_name: String,
    pub text: String,
    pub attachments: Vec<Attachment>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attachment {
    pub kind: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Tells the dispatcher where and how to reply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyContext {
    pub chat_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
}

/// Direction of a channel message record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Inbound => "inbound",
            Self::Outbound => "outbound",
        }
    }
}
