use serde::Deserialize;
use serde::Serialize;

pub use crate::storage::dal::usage::types::CostSummary;

/// Model role — what functional purpose the LLM call served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    #[default]
    Reasoning,
    Compaction,
    Checkpoint,
}

impl ModelRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Reasoning => "reasoning",
            Self::Compaction => "compaction",
            Self::Checkpoint => "checkpoint",
        }
    }
}

impl std::fmt::Display for ModelRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub agent_id: String,
    pub user_id: String,
    pub session_id: String,
    pub run_id: String,
    pub provider: String,
    pub model: String,
    pub model_role: ModelRole,
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub ttft_ms: u64,
    pub cost: f64,
}

#[derive(Debug, Clone)]
pub enum UsageScope {
    User { user_id: String },
    AgentTotal { agent_id: String },
    AgentDaily { agent_id: String, day: String },
}
