use std::path::PathBuf;
use std::sync::Arc;

use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::prompt_model::PromptConfig;
use crate::kernel::run::prompt::prompt_model::PromptSeed;
use crate::kernel::run::prompt::prompt_model::PromptVariable;
use crate::kernel::run::prompt::CloudPromptLoader;
use crate::kernel::run::prompt::LocalPromptResolver;
use crate::kernel::run::prompt::PromptResolver;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::tools::definition::tool_definition::ToolDefinition;

pub fn build_local_prompt_resolver(
    tools: Arc<Vec<ToolDefinition>>,
    cwd: PathBuf,
) -> Arc<dyn PromptResolver> {
    Arc::new(LocalPromptResolver::new(PromptSeed::default(), tools, cwd))
}

pub struct CloudPromptResolverConfig {
    pub storage: Arc<AgentStore>,
    pub org: Arc<OrgServices>,
    pub tools: Arc<Vec<ToolDefinition>>,
    pub variables: Vec<PromptVariable>,
    pub prompt_config: Option<PromptConfig>,
    pub cwd: PathBuf,
    pub cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    pub directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
    pub memory_enabled: bool,
    pub memory_recall_budget: usize,
    pub agent_id: String,
    pub user_id: String,
    pub session_id: String,
}

pub fn build_cloud_prompt_resolver(cfg: CloudPromptResolverConfig) -> Arc<dyn PromptResolver> {
    Arc::new(CloudPromptLoader::new(
        cfg.storage,
        cfg.org,
        cfg.tools,
        cfg.variables,
        cfg.prompt_config,
        cfg.cwd,
        cfg.cluster_client,
        cfg.directive,
        cfg.memory_enabled,
        cfg.memory_recall_budget,
        cfg.agent_id,
        cfg.user_id,
        cfg.session_id,
    ))
}
