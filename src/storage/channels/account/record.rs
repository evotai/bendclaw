use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelAccountRecord {
    pub id: String,
    pub channel_type: String,
    pub account_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub scope: String,
    pub node_id: String,
    pub created_by: String,
    pub config: serde_json::Value,
    pub enabled: bool,
    pub lease_node_id: Option<String>,
    pub lease_token: Option<String>,
    pub lease_expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
