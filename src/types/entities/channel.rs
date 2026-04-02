use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub channel_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub kind: String,
    #[serde(default)]
    pub config: serde_json::Value,
    #[serde(default)]
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
}
