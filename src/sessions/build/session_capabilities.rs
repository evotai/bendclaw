use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::config::agent::AgentConfig;
use crate::kernel::directive::DirectiveService;
use crate::kernel::run::planning::PromptConfig;
use crate::kernel::run::planning::PromptResolver;
use crate::kernel::tools::definition::toolset::Toolset;
use crate::llm::provider::LLMProvider;
use crate::sessions::backend::context::SessionContextProvider;
use crate::sessions::backend::sink::RunInitializer;
use crate::sessions::workspace::Workspace;
use crate::types::Result;

/// Owner identity for session assembly. Callers (invocation layer, factory) construct this.
pub struct SessionOwner {
    pub agent_id: String,
    pub user_id: String,
}

/// Session-scoped labels for logging and run records (agent + user + session).
///
/// Distinct from `tools::run_labels::RunLabels` which is per-run and includes
/// trace_id + run_id for the tool runtime layer.
#[derive(Debug, Clone)]
pub struct RunLabels {
    pub agent_id: Arc<str>,
    pub user_id: Arc<str>,
    pub session_id: Arc<str>,
}

/// The single product of both assemblers. Session doesn't care how it was built.
pub struct SessionAssembly {
    pub labels: RunLabels,
    pub core: SessionCore,
    pub infra: RuntimeInfra,
    pub agent: AgentContext,
}

/// Session-essential dependencies: workspace, LLM, tools, prompt, backend.
pub struct SessionCore {
    pub workspace: Arc<Workspace>,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub toolset: Toolset,
    pub prompt_resolver: Arc<dyn PromptResolver>,
    pub context_provider: Arc<dyn SessionContextProvider>,
    pub run_initializer: Arc<dyn RunInitializer>,
}

/// Infrastructure: storage, writers, tracing.
pub struct RuntimeInfra {
    pub store: Arc<dyn crate::sessions::store::SessionStore>,
    pub trace_factory: Arc<dyn crate::kernel::trace::factory::TraceFactory>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::kernel::run::persist::persist_op::PersistWriter,
}

/// Agent-level context: org, config, prompt data, optional services.
pub struct AgentContext {
    pub org: Arc<dyn crate::kernel::runtime::session_org::SessionOrgServices>,
    pub config: Arc<AgentConfig>,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<DirectiveService>>,
    pub prompt_config: Option<PromptConfig>,
    pub prompt_variables: Vec<crate::kernel::run::planning::PromptVariable>,
    pub skill_executor: Arc<dyn crate::kernel::run::execution::skills::SkillExecutor>,
    pub memory_recaller: Option<Arc<dyn MemoryRecaller>>,
}

/// Runtime memory recall. Persistent sessions have one; ephemeral don't.
#[async_trait]
pub trait MemoryRecaller: Send + Sync {
    async fn recall(&self, query: &str) -> Result<Option<String>>;
}
