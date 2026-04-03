use serde::Deserialize;
use serde::Serialize;

/// A channel account binds a channel type + credentials to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccount {
    pub channel_account_id: String, // DB PK
    pub channel_type: String,
    pub external_account_id: String, // platform-side identifier
    pub agent_id: String,
    pub user_id: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub created_at: String,
    pub updated_at: String,
}
