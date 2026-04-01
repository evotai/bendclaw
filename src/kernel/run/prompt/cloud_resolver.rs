use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use super::loader::CloudPromptLoader;
use super::model::*;
use super::prompt_contract::PromptResolver;
use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::kernel::runtime::org::OrgServices;
use crate::llm::tool::ToolSchema;

pub struct CloudPromptResolver {
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
}

impl CloudPromptResolver {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
    ) -> Self {
        Self {
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
        }
    }
}

#[async_trait]
impl PromptResolver for CloudPromptResolver {
    async fn resolve(&self, meta: &PromptRequestMeta) -> Result<String> {
        let directive_prompt = self.directive.as_ref().and_then(|d| d.cached_prompt());

        let mut pb = CloudPromptLoader::new(self.storage.clone(), self.org.skills().clone())
            .with_tools(self.tools.clone())
            .with_variables(self.variables.clone())
            .with_cached_config(self.prompt_config.clone())
            .with_cwd(self.cwd.clone());

        if let Some(ref cc) = self.cluster_client {
            pb = pb.with_cluster_client(cc.clone());
        }
        pb = pb.with_directive_prompt(directive_prompt);

        let recall_memory = self.org.memory().filter(|_| self.memory_enabled).cloned();
        pb = pb.with_memory_service(recall_memory, self.memory_recall_budget);
        pb = pb.with_overlays(meta.system_overlay.clone(), meta.skill_overlay.clone());

        pb.build(&self.agent_id, &self.user_id, &self.session_id)
            .await
    }
}
