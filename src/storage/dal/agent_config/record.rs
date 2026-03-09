use std::collections::HashMap;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigRecord {
    pub agent_id: String,
    pub system_prompt: String,
    pub display_name: String,
    pub description: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    pub created_at: String,
    pub updated_at: String,
}
