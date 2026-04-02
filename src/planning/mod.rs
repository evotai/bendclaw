//! Planning: prompt assembly, context preparation, and strategy decisions.
//!
//! Transforms a bound run context into a concrete execution plan:
//! system prompt, tool configuration, context window, and run parameters.
//!
//! Pipeline position: **third stage** — consumes `binding/` output, feeds `execution/`.

use async_trait::async_trait;

use crate::types::Result;

/// Canonical contract: assemble a prompt and execution plan from a bound context.
///
/// Implementations resolve the system prompt (cloud or local), apply tool
/// prompt sections, manage context projection, and produce a plan that
/// the executor can run directly.
#[async_trait]
pub trait RunPlanner: Send + Sync {
    type Binding;
    type Plan;

    async fn plan(&self, binding: Self::Binding) -> Result<Self::Plan>;
}
