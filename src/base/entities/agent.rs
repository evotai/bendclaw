use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Agent {
    pub agent_id: String,
    pub user_id: String,
    pub name: String,
    pub model: String,
    #[serde(default)]
    pub config: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}
