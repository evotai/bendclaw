use super::orchestrator::compact_messages;
use super::types::CompactionResult;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::AgentMessage;

pub trait CompactionStrategy: Send + Sync {
    fn compact(
        &self,
        messages: Vec<AgentMessage>,
        config: &ContextConfig,
        budget_state: &CompactionBudgetState,
    ) -> CompactionResult;
}

pub struct DefaultCompaction;

impl CompactionStrategy for DefaultCompaction {
    fn compact(
        &self,
        messages: Vec<AgentMessage>,
        config: &ContextConfig,
        budget_state: &CompactionBudgetState,
    ) -> CompactionResult {
        compact_messages(messages, config, budget_state)
    }
}
