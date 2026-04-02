//! Execution: the turn loop — LLM inference, tool dispatch, skill dispatch,
//! context compaction, and run persistence.
//!
//! This is the core engine. It takes a plan from `planning/` and runs the
//! multi-turn LLM loop until completion, cancellation, or error.
//!
//! Pipeline position: **fourth stage** — consumes `planning/` output, emits events to `result/`.

use async_trait::async_trait;

use crate::base::Result;

/// Canonical contract: execute a planned run and produce a result.
///
/// Implementations own the turn loop: call LLM, dispatch tools/skills,
/// handle compaction, emit events, and persist checkpoints.
#[async_trait]
pub trait RunExecutor: Send + Sync {
    type Plan;
    type Output;

    async fn execute(&self, plan: Self::Plan) -> Result<Self::Output>;
}
