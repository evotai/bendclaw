use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub scope: String,
    #[serde(default)]
    pub state: serde_json::Value,
    #[serde(default)]
    pub meta: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
