use serde::Deserialize;
use serde::Serialize;

use crate::llm::config::LLMConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigVersionRecord {
    pub id: String,
    pub agent_id: String,
    pub version: u32,
    pub label: String,
    pub stage: String,
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    pub llm_config: Option<LLMConfig>,
    pub notes: String,
    pub created_at: String,
}
