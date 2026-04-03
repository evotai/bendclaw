//! Planning: prompt assembly, context preparation, and strategy decisions.
//!
//! Transforms a bound run context into a concrete execution plan:
//! system prompt, tool configuration, context window, and run parameters.
//!
//! Pipeline position: **third stage** — consumes `binding/` output, feeds `execution/`.

pub mod prompt_contract;
pub mod prompt_diagnostics;
pub mod prompt_loader;
pub mod prompt_local;
pub mod prompt_model;
pub mod prompt_projection;
pub mod prompt_renderer;
pub mod run_builder;
pub mod run_deps;
pub mod tool_prompt;
pub mod tool_view;

use async_trait::async_trait;
pub use prompt_contract::PromptResolver;
pub use prompt_loader::CloudPromptLoader;
pub use prompt_local::LocalPromptResolver;
pub use prompt_model::substitute_template;
pub use prompt_model::truncate_layer;
pub use prompt_model::PromptConfig;
pub use prompt_model::PromptInputs;
pub use prompt_model::PromptRequestMeta;
pub use prompt_model::PromptSeed;
pub use prompt_model::PromptVariable;
pub use prompt_model::SkillPromptEntry;
pub use prompt_model::MAX_ERRORS_BYTES;
pub use prompt_model::MAX_IDENTITY_BYTES;
pub use prompt_model::MAX_RUNTIME_BYTES;
pub use prompt_model::MAX_SKILLS_BYTES;
pub use prompt_model::MAX_SOUL_BYTES;
pub use prompt_model::MAX_SYSTEM_BYTES;
pub use prompt_model::MAX_TOOLS_BYTES;
pub use prompt_model::MAX_VARIABLES_BYTES;
pub use prompt_renderer::build_prompt;
pub use run_builder::build_run_driver;
pub use run_builder::RunConfig;
pub use run_builder::RunDriver;
pub use run_builder::RunRequest;
pub use run_deps::RunDeps;

use crate::types::Result;

/// Canonical contract: assemble a prompt and execution plan from a bound context.
#[async_trait]
pub trait RunPlanner: Send + Sync {
    type Binding;
    type Plan;

    async fn plan(&self, binding: Self::Binding) -> Result<Self::Plan>;
}
