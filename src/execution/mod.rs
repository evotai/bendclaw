//! Execution: the turn loop — LLM inference, tool dispatch, skill dispatch,
//! context compaction, and run persistence.
//!
//! This is the core engine. It takes a plan from `planning/` and runs the
//! multi-turn LLM loop until completion, cancellation, or error.
//!
//! Pipeline position: **fourth stage** — consumes `planning/` output, emits events to `result/`.

use async_trait::async_trait;

// --- Sub-modules ---
pub mod checkpoint;
pub mod compaction;
pub mod context;
pub mod default_identity;
pub mod event;
pub mod fmt;
pub mod hooks;
pub mod inbox;
pub mod launcher;
pub mod llm;
pub mod memory;
pub mod persist;
pub mod result;
pub(crate) mod run_record;
pub mod runtime_context;
pub mod skills;
pub mod tools;
pub mod usage;

// --- Re-exports: top-level run types ---
pub use checkpoint::CompactionCheckpoint;
// --- Re-exports: compaction ---
pub use compaction::build_transcript_from;
pub use compaction::split_chunks;
pub use compaction::CompactionConfig;
pub use compaction::CompactionOutcome;
pub use compaction::CompactionPlan;
pub use compaction::CompactionResult;
pub use compaction::CompactionStrategy;
pub use compaction::Compactor;
pub use compaction::TieredCompactionStrategy;
pub use event::Delta;
pub use event::Event;
// --- Re-exports: llm ---
pub use llm::turn_engine::Engine;
pub use result::ContentBlock;
pub use result::Reason;
pub use result::Result;
pub use result::Usage;
// --- Re-exports: skills ---
pub use skills::parse_skill_args;
pub use skills::NoopSkillExecutor;
pub use skills::SkillError;
pub use skills::SkillExecutor;
pub use skills::SkillOutput;
pub use skills::SkillRunner;
pub use skills::UsageSink;
// --- Re-exports: tools ---
pub use tools::tool_orchestrator::ToolDispatchOutput;
pub use tools::tool_orchestrator::ToolOrchestrator;
pub use tools::tool_stack::ToolStack;
pub use tools::tool_stack::ToolStackConfig;
pub use tools::turn_context::TurnContext;

/// Canonical contract: execute a planned run and produce a result.
#[async_trait]
pub trait RunExecutor: Send + Sync {
    type Plan;
    type Output;

    async fn execute(&self, plan: Self::Plan) -> crate::types::Result<Self::Output>;
}
