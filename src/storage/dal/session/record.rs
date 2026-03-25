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
    pub scope: String,
    #[serde(default)]
    pub base_key: String,
    #[serde(default)]
    pub replaced_by_session_id: String,
    #[serde(default)]
    pub reset_reason: String,
    #[serde(default)]
    pub session_state: serde_json::Value,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
