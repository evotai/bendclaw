use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersionRecord {
    pub id: String,
    pub agent_id: String,
    pub version: u32,
    pub label: String,
    pub stage: String,
    pub system_prompt: String,
    pub display_name: String,
    pub description: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    pub notes: String,
    pub created_at: String,
}
