use std::path::PathBuf;
use std::sync::Arc;

use crate::kernel::agent_store::AgentStore;
use crate::kernel::run::prompt::model::PromptConfig;
use crate::kernel::run::prompt::model::PromptSeed;
use crate::kernel::run::prompt::model::PromptVariable;
use crate::kernel::run::prompt::CloudPromptResolver;
use crate::kernel::run::prompt::LocalPromptResolver;
use crate::kernel::run::prompt::PromptResolver;
use crate::kernel::runtime::org::OrgServices;
use crate::llm::tool::ToolSchema;

pub fn build_local_prompt_resolver(
    tools: Arc<Vec<ToolSchema>>,
    cwd: PathBuf,
) -> Arc<dyn PromptResolver> {
    Arc::new(LocalPromptResolver::new(PromptSeed::default(), tools, cwd))
}

#[allow(clippy::too_many_arguments)]
pub fn build_cloud_prompt_resolver(
    storage: Arc<AgentStore>,
    org: Arc<OrgServices>,
    tools: Arc<Vec<ToolSchema>>,
    variables: Vec<PromptVariable>,
    prompt_config: Option<PromptConfig>,
    cwd: PathBuf,
    cluster_client: Option<Arc<crate::kernel::cluster::ClusterService>>,
    directive: Option<Arc<crate::kernel::directive::DirectiveService>>,
    memory_enabled: bool,
    memory_recall_budget: usize,
    agent_id: String,
    user_id: String,
    session_id: String,
) -> Arc<dyn PromptResolver> {
    Arc::new(CloudPromptResolver::new(
        storage,
        org,
        tools,
        variables,
        prompt_config,
        cwd,
        cluster_client,
        directive,
        memory_enabled,
        memory_recall_budget,
        agent_id,
        user_id,
        session_id,
    ))
}
