use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CostSummary {
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_reasoning_tokens: u64,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub record_count: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_write_tokens: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DailyUsage {
    pub date: String,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
    pub cost: f64,
    pub requests: u64,
}
