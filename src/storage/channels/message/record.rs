use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessageRecord {
    pub id: String,
    pub channel_type: String,
    pub account_id: String,
    pub chat_id: String,
    pub session_id: String,
    pub direction: String,
    pub sender_id: String,
    pub text: String,
    pub platform_message_id: String,
    pub run_id: String,
    pub attachments: String,
    pub created_at: String,
}
