use serde::Deserialize;
use serde::Serialize;

/// A single usage record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub session_id: String,
    pub run_id: String,
    pub provider: String,
    pub model: String,
    pub model_role: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub ttft_ms: u64,
    pub cost: f64,
    pub created_at: String,
}
