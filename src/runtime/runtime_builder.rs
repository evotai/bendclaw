use std::sync::Arc;

use crate::cluster::ClusterOptions;
use crate::config::agent::AgentConfig;
use crate::config::agent::CheckpointConfig;
use crate::config::ClusterConfig;
use crate::config::DirectiveConfig;
use crate::config::WorkspaceConfig;
use crate::llm::provider::LLMProvider;
use crate::runtime::runtime::Runtime;
use crate::runtime::runtime_bootstrap::construct;
use crate::runtime::runtime_bootstrap::construct_minimal;
use crate::storage::pool::Pool;
use crate::types::Result;

pub struct Builder {
    api_base_url: String,
    api_token: String,
    warehouse: String,
    db_prefix: String,
    node_id: String,
    llm: Arc<dyn LLMProvider>,
    root_pool: Option<Pool>,
    hub_config: Option<crate::config::HubConfig>,
    skills_sync_interval_secs: u64,
    max_iterations: u32,
    max_context_tokens: usize,
    max_duration_secs: u64,
    workspace: WorkspaceConfig,
    cluster_config: Option<ClusterConfig>,
    cluster_options: ClusterOptions,
    auth_key: String,
    directive_config: Option<DirectiveConfig>,
}

impl Builder {
    pub(crate) fn new(
        api_base_url: &str,
        api_token: &str,
        warehouse: &str,
        db_prefix: &str,
        node_id: &str,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        Self {
            api_base_url: api_base_url.to_string(),
            api_token: api_token.to_string(),
            warehouse: warehouse.to_string(),
            db_prefix: db_prefix.to_string(),
            node_id: node_id.to_string(),
            llm,
            root_pool: None,
            hub_config: None,
            skills_sync_interval_secs: 30,
            max_iterations: 20,
            max_context_tokens: 250_000,
            max_duration_secs: 300,
            workspace: WorkspaceConfig::default(),
            cluster_config: None,
            cluster_options: ClusterOptions::default(),
            auth_key: String::new(),
            directive_config: None,
        }
    }

    #[must_use]
    pub fn with_hub_config(mut self, hub_config: Option<crate::config::HubConfig>) -> Self {
        self.hub_config = hub_config;
        self
    }

    #[must_use]
    pub fn with_skills_sync_interval(mut self, sync_interval_secs: u64) -> Self {
        self.skills_sync_interval_secs = sync_interval_secs;
        self
    }

    #[must_use]
    pub fn with_max_iterations(mut self, n: u32) -> Self {
        self.max_iterations = n;
        self
    }

    #[must_use]
    pub fn with_max_context_tokens(mut self, n: usize) -> Self {
        self.max_context_tokens = n;
        self
    }

    #[must_use]
    pub fn with_max_duration_secs(mut self, s: u64) -> Self {
        self.max_duration_secs = s;
        self
    }

    #[must_use]
    pub fn with_workspace(mut self, config: WorkspaceConfig) -> Self {
        self.workspace = config;
        self
    }

    #[must_use]
    pub fn with_root_pool(mut self, pool: Pool) -> Self {
        self.root_pool = Some(pool);
        self
    }

    #[must_use]
    pub fn with_cluster_config(
        mut self,
        cluster_config: Option<ClusterConfig>,
        auth_key: &str,
    ) -> Self {
        self.cluster_config = cluster_config;
        self.auth_key = auth_key.to_string();
        self
    }

    #[must_use]
    pub fn with_cluster_options(mut self, cluster_options: ClusterOptions) -> Self {
        self.cluster_options = cluster_options;
        self
    }

    #[must_use]
    pub fn with_directive_config(mut self, directive_config: Option<DirectiveConfig>) -> Self {
        self.directive_config = directive_config;
        self
    }

    pub async fn build(self) -> Result<Arc<Runtime>> {
        let config = self.build_config();
        construct(
            config,
            self.llm,
            self.root_pool,
            self.hub_config,
            self.skills_sync_interval_secs,
            self.cluster_config,
            self.cluster_options,
            self.auth_key,
            self.directive_config,
        )
        .await
    }

    pub async fn build_minimal(self) -> Result<Arc<Runtime>> {
        let config = self.build_config();
        construct_minimal(config, self.llm, self.root_pool).await
    }

    fn build_config(&self) -> AgentConfig {
        AgentConfig {
            node_id: self.node_id.clone(),
            databend_api_base_url: self.api_base_url.clone(),
            databend_api_token: self.api_token.clone(),
            databend_warehouse: self.warehouse.clone(),
            db_prefix: self.db_prefix.clone(),
            max_iterations: self.max_iterations,
            max_context_tokens: self.max_context_tokens,
            max_duration_secs: self.max_duration_secs,
            workspace: self.workspace.clone(),
            checkpoint: CheckpointConfig::default(),
            memory: crate::config::agent::MemoryConfig::default(),
        }
    }
}
