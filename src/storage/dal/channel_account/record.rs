use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccountRecord {
    pub id: String,
    pub channel_type: String,
    pub account_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}
