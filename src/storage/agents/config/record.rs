use serde::Deserialize;
use serde::Serialize;

use crate::llm::config::LLMConfig;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfigRecord {
    pub agent_id: String,
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
    pub llm_config: Option<LLMConfig>,
    pub updated_at: String,
}
