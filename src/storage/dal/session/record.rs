use serde::Deserialize;
use serde::Serialize;

/// Persisted session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub title: String,
    #[serde(default)]
    pub session_state: serde_json::Value,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
