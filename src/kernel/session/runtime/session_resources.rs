use std::sync::Arc;

use parking_lot::RwLock;

use crate::kernel::directive::DirectiveService;
use crate::kernel::run::hooks::BeforeTurnHook;
use crate::kernel::run::hooks::SteeringSource;
use crate::kernel::run::prompt::PromptConfig;
use crate::kernel::run::prompt::PromptResolver;
use crate::kernel::run::prompt::PromptVariable;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::session_org::SessionOrgServices;
use crate::kernel::session::backend::context::SessionContextProvider;
use crate::kernel::session::backend::sink::RunInitializer;
use crate::kernel::session::store::SessionStore;
use crate::kernel::session::workspace::Workspace;
use crate::kernel::skills::execution::SkillExecutor;
use crate::kernel::tools::definition::toolset::Toolset;
use crate::kernel::trace::factory::TraceFactory;
use crate::llm::provider::LLMProvider;

pub struct SessionResources {
    pub workspace: Arc<Workspace>,
    pub toolset: Toolset,
    pub org: Arc<dyn SessionOrgServices>,
    pub store: Arc<dyn SessionStore>,
    pub llm: Arc<RwLock<Arc<dyn LLMProvider>>>,
    pub config: Arc<AgentConfig>,
    pub prompt_variables: Vec<PromptVariable>,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<DirectiveService>>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub trace_factory: Arc<dyn TraceFactory>,
    pub persist_writer: crate::kernel::run::persist_op::PersistWriter,
    pub prompt_config: Option<PromptConfig>,
    pub before_turn_hook: Option<Arc<dyn BeforeTurnHook>>,
    pub steering_source: Option<Arc<dyn SteeringSource>>,
    pub prompt_resolver: Arc<dyn PromptResolver>,
    pub context_provider: Arc<dyn SessionContextProvider>,
    pub run_initializer: Arc<dyn RunInitializer>,
    pub skill_executor: Arc<dyn SkillExecutor>,
}
