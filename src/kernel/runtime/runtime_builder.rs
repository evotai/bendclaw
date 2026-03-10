use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::config::WorkspaceConfig;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::dispatch::dispatch_inbound;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::agent_config::CheckpointConfig;
use crate::kernel::runtime::runtime::Runtime;
use crate::kernel::runtime::runtime::RuntimeParts;
use crate::kernel::runtime::runtime::RuntimeStatus;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

pub struct Builder {
    api_base_url: String,
    api_token: String,
    warehouse: String,
    db_prefix: String,
    llm: Arc<dyn LLMProvider>,
    skills_dir: String,
    skills_sync_interval_secs: u64,
    max_iterations: u32,
    max_context_tokens: usize,
    max_duration_secs: u64,
    workspace: WorkspaceConfig,
}

impl Builder {
    pub(crate) fn new(
        api_base_url: &str,
        api_token: &str,
        warehouse: &str,
        db_prefix: &str,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        Self {
            api_base_url: api_base_url.to_string(),
            api_token: api_token.to_string(),
            warehouse: warehouse.to_string(),
            db_prefix: db_prefix.to_string(),
            llm,
            skills_dir: "./skills".to_string(),
            skills_sync_interval_secs: 30,
            max_iterations: 20,
            max_context_tokens: 250_000,
            max_duration_secs: 300,
            workspace: WorkspaceConfig::default(),
        }
    }

    #[must_use]
    pub fn with_skills_dir(mut self, dir: &str) -> Self {
        self.skills_dir = dir.to_string();
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

    pub async fn build(self) -> Result<Arc<Runtime>> {
        let config = AgentConfig {
            databend_api_base_url: self.api_base_url,
            databend_api_token: self.api_token,
            databend_warehouse: self.warehouse,
            db_prefix: self.db_prefix,
            skills_dir: self.skills_dir,
            max_iterations: self.max_iterations,
            max_context_tokens: self.max_context_tokens,
            max_duration_secs: self.max_duration_secs,
            workspace: self.workspace,
            checkpoint: CheckpointConfig::default(),
        };
        construct(config, self.llm, self.skills_sync_interval_secs).await
    }
}

async fn construct(
    config: AgentConfig,
    llm: Arc<dyn LLMProvider>,
    skills_sync_interval_secs: u64,
) -> Result<Arc<Runtime>> {
    let t0 = std::time::Instant::now();

    let pool = Pool::new(
        &config.databend_api_base_url,
        &config.databend_api_token,
        &config.databend_warehouse,
    )?;
    let databases = Arc::new(crate::storage::AgentDatabases::new(
        pool.clone(),
        &config.db_prefix,
    )?);
    tracing::info!(elapsed_ms = t0.elapsed().as_millis() as u64, "pool created");

    let sync_cancel = CancellationToken::new();

    let skills_path = Path::new(&config.skills_dir);
    let t2 = std::time::Instant::now();
    let (skills, skill_count, sync_handle) = build_skill_catalog(
        databases.clone(),
        skills_path,
        skills_sync_interval_secs,
        sync_cancel.clone(),
    )
    .await;
    tracing::info!(
        elapsed_ms = t2.elapsed().as_millis() as u64,
        skills = skill_count,
        "skill catalog ready"
    );

    tracing::info!(
        total_ms = t0.elapsed().as_millis() as u64,
        skills = skill_count,
        "runtime ready"
    );

    let sessions = Arc::new(SessionManager::new());
    let channels = Arc::new(build_channel_registry());

    let scheduler_handle = crate::kernel::scheduler::TaskScheduler::spawn(
        databases.clone(),
        sync_cancel.clone(),
        reqwest::Client::new(),
    );

    // Use Arc::new_cyclic so the supervisor's event_handler can capture a Weak<Runtime>.
    let runtime = Arc::new_cyclic(|weak: &std::sync::Weak<Runtime>| {
        let weak = weak.clone();
        let event_handler: Arc<dyn Fn(ChannelAccount, InboundEvent) + Send + Sync> =
            Arc::new(move |account, event| {
                if let Some(runtime) = weak.upgrade() {
                    tokio::spawn(async move {
                        dispatch_inbound(&runtime, account, event).await;
                    });
                }
            });
        let supervisor = Arc::new(ChannelSupervisor::new(channels.clone(), event_handler));

        Runtime::from_parts(RuntimeParts {
            sessions,
            channels,
            supervisor,
            config,
            databases,
            llm: RwLock::new(llm),
            skills,
            status: RwLock::new(RuntimeStatus::Ready),
            sync_cancel,
            sync_handle: RwLock::new(Some(sync_handle)),
            scheduler_handle: RwLock::new(Some(scheduler_handle)),
        })
    });

    Ok(runtime)
}

async fn build_skill_catalog(
    databases: Arc<crate::storage::AgentDatabases>,
    skills_path: &Path,
    sync_interval_secs: u64,
    cancel: CancellationToken,
) -> (Arc<dyn SkillCatalog>, usize, tokio::task::JoinHandle<()>) {
    let catalog = Arc::new(crate::kernel::skills::catalog::SkillCatalogImpl::new(
        databases,
        skills_path.to_path_buf(),
    ));

    if let Err(e) = catalog.load().await {
        tracing::warn!(error = %e, "initial skill sync from Databend failed, starting with empty catalog");
    } else {
        tracing::info!("skills synced from Databend");
    }

    catalog.log_loaded_skills();
    let skill_count = catalog.loaded_skills().len();

    let sync_handle = crate::kernel::skills::catalog::spawn_sync_task(
        catalog.clone(),
        sync_interval_secs,
        cancel,
    );

    (catalog, skill_count, sync_handle)
}

fn build_channel_registry() -> crate::kernel::channel::registry::ChannelRegistry {
    use crate::kernel::channel::plugins::feishu::FeishuChannel;
    use crate::kernel::channel::plugins::github::GitHubChannel;
    use crate::kernel::channel::plugins::http_api::HttpApiChannel;
    use crate::kernel::channel::plugins::telegram::TelegramChannel;

    let mut registry = crate::kernel::channel::registry::ChannelRegistry::new();
    registry.register(Arc::new(HttpApiChannel::new()));
    registry.register(Arc::new(TelegramChannel::new()));
    registry.register(Arc::new(FeishuChannel::new()));
    registry.register(Arc::new(GitHubChannel::new()));
    registry
}
