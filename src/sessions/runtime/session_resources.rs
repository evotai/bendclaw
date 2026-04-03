use std::sync::Arc;

use parking_lot::RwLock;

use crate::config::agent::AgentConfig;
use crate::execution::hooks::BeforeTurnHook;
use crate::execution::hooks::SteeringSource;
use crate::execution::skills::SkillExecutor;
use crate::kernel::directive::DirectiveService;
use crate::kernel::runtime::session_org::SessionOrgServices;
use crate::llm::provider::LLMProvider;
use crate::planning::PromptConfig;
use crate::planning::PromptResolver;
use crate::planning::PromptVariable;
use crate::sessions::backend::context::SessionContextProvider;
use crate::sessions::backend::sink::RunInitializer;
use crate::sessions::store::SessionStore;
use crate::sessions::workspace::Workspace;
use crate::tools::definition::toolset::Toolset;
use crate::traces::factory::TraceFactory;

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
    pub trace_writer: crate::traces::TraceWriter,
    pub trace_factory: Arc<dyn TraceFactory>,
    pub persist_writer: crate::execution::persist::persist_op::PersistWriter,
    pub prompt_config: Option<PromptConfig>,
    pub before_turn_hook: Option<Arc<dyn BeforeTurnHook>>,
    pub steering_source: Option<Arc<dyn SteeringSource>>,
    pub prompt_resolver: Arc<dyn PromptResolver>,
    pub context_provider: Arc<dyn SessionContextProvider>,
    pub run_initializer: Arc<dyn RunInitializer>,
    pub skill_executor: Arc<dyn SkillExecutor>,
}
