use async_trait::async_trait;

use crate::kernel::run::checkpoint::CompactionCheckpoint;
use crate::kernel::Message;
use crate::llm::usage::TokenUsage;

/// Outcome of a compaction pass.
pub struct CompactionOutcome {
    pub messages: Vec<Message>,
    pub token_usage: TokenUsage,
    pub description: String,
    pub checkpoint: Option<CompactionCheckpoint>,
}

/// Configuration for compaction behavior.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    pub max_context_tokens: usize,
    pub keep_recent: usize,
    pub keep_first: usize,
    pub tool_output_max_lines: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 250_000,
            keep_recent: 10,
            keep_first: 2,
            tool_output_max_lines: 80,
        }
    }
}

/// Pluggable compaction strategy.
///
/// Responsible for: given a message list and budget, return a compacted list.
/// Not responsible for: trigger decisions, failure tracking, cooldown
/// (those are managed by `Compactor`).
#[async_trait]
pub trait CompactionStrategy: Send + Sync {
    async fn compact(
        &self,
        messages: Vec<Message>,
        config: &CompactionConfig,
        current_run_id: &str,
    ) -> Option<CompactionOutcome>;
}
