use std::path::PathBuf;
use std::sync::Arc;

use crate::agent_store::AgentStore;
use crate::planning::prompt_model::PromptConfig;
use crate::planning::prompt_model::PromptSeed;
use crate::planning::prompt_model::PromptVariable;
use crate::planning::CloudPromptLoader;
use crate::planning::LocalPromptResolver;
use crate::planning::PromptResolver;
use crate::runtime::org::OrgServices;
use crate::tools::definition::tool_definition::ToolDefinition;

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
    pub cluster_client: Option<Arc<crate::cluster::ClusterService>>,
    pub directive: Option<Arc<crate::directive::DirectiveService>>,
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
