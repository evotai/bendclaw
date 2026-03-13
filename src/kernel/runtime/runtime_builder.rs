use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::base::Result;
use crate::client::BendclawClient;
use crate::client::ClusterClient;
use crate::client::DirectiveClient;
use crate::config::ClusterConfig;
use crate::config::DirectiveConfig;
use crate::config::WorkspaceConfig;
use crate::kernel::channel::account::ChannelAccount;
use crate::kernel::channel::dispatch::dispatch_inbound;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterOptions;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::agent_config::CheckpointConfig;
use crate::kernel::runtime::runtime::Runtime;
use crate::kernel::runtime::runtime::RuntimeParts;
use crate::kernel::runtime::runtime::RuntimeStatus;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::store::SkillStore;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

pub struct Builder {
    api_base_url: String,
    api_token: String,
    warehouse: String,
    db_prefix: String,
    instance_id: String,
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
        instance_id: &str,
        llm: Arc<dyn LLMProvider>,
    ) -> Self {
        Self {
            api_base_url: api_base_url.to_string(),
            api_token: api_token.to_string(),
            warehouse: warehouse.to_string(),
            db_prefix: db_prefix.to_string(),
            instance_id: instance_id.to_string(),
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
        let config = AgentConfig {
            instance_id: self.instance_id,
            databend_api_base_url: self.api_base_url,
            databend_api_token: self.api_token,
            databend_warehouse: self.warehouse,
            db_prefix: self.db_prefix,
            max_iterations: self.max_iterations,
            max_context_tokens: self.max_context_tokens,
            max_duration_secs: self.max_duration_secs,
            workspace: self.workspace,
            checkpoint: CheckpointConfig::default(),
        };
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
}

#[allow(clippy::too_many_arguments)]
async fn construct(
    config: AgentConfig,
    llm: Arc<dyn LLMProvider>,
    root_pool: Option<Pool>,
    hub_config: Option<crate::config::HubConfig>,
    skills_sync_interval_secs: u64,
    cluster_config: Option<ClusterConfig>,
    cluster_options: ClusterOptions,
    auth_key: String,
    directive_config: Option<DirectiveConfig>,
) -> Result<Arc<Runtime>> {
    let t0 = std::time::Instant::now();

    let pool = match root_pool {
        Some(pool) => pool,
        None => Pool::new(
            &config.databend_api_base_url,
            &config.databend_api_token,
            &config.databend_warehouse,
        )?,
    };
    let databases = Arc::new(crate::storage::AgentDatabases::new(
        pool.clone(),
        &config.db_prefix,
    )?);
    tracing::info!(elapsed_ms = t0.elapsed().as_millis() as u64, "pool created");

    let sync_cancel = CancellationToken::new();

    let workspace_root = Path::new(&config.workspace.root_dir);
    let t2 = std::time::Instant::now();
    let (skills, skill_count, sync_handle) = build_skill_store(
        databases.clone(),
        workspace_root,
        hub_config,
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

    // Directive cache (opt-in)
    let (directive, directive_handle) = if let Some(dc) = directive_config {
        let client = Arc::new(DirectiveClient::new(&dc.api_base, &dc.token)?);
        let service = Arc::new(DirectiveService::new(
            client,
            DirectiveService::DEFAULT_REFRESH_INTERVAL,
        ));
        if let Err(e) = service.refresh().await {
            tracing::warn!(error = %e, "directive init failed, starting with empty cache");
        }
        let handle = service.spawn_refresh_loop(sync_cancel.clone());
        (Some(service), Some(handle))
    } else {
        (None, None)
    };

    // Cluster initialization (opt-in)
    let (cluster_service, heartbeat_handle) = if let Some(cc) = cluster_config {
        let cluster_client = Arc::new(ClusterClient::new(
            &cc.registry_url,
            &cc.registry_token,
            &config.instance_id,
            &cc.advertise_url,
        ));
        let bendclaw_client = Arc::new(BendclawClient::new(
            &auth_key,
            std::time::Duration::from_secs(300),
        ));
        let svc = Arc::new(ClusterService::with_options(
            cluster_client,
            bendclaw_client,
            cluster_options,
        ));

        if let Err(e) = svc.register_and_discover().await {
            tracing::warn!(error = %e, "cluster init failed, continuing without cluster");
            (None, None)
        } else {
            let hb_handle = svc.spawn_heartbeat(sync_cancel.clone());
            (Some(svc), Some(hb_handle))
        }
    } else {
        (None, None)
    };

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
            sync_cancel: sync_cancel.clone(),
            sync_handle: RwLock::new(Some(sync_handle)),
            scheduler_handle: RwLock::new(None),
            cluster: cluster_service,
            heartbeat_handle: RwLock::new(heartbeat_handle),
            directive,
            directive_handle: RwLock::new(directive_handle),
        })
    });

    let scheduler_handle = crate::kernel::scheduler::TaskScheduler::spawn(
        runtime.clone(),
        sync_cancel,
        reqwest::Client::new(),
    );
    *runtime.scheduler_handle.write() = Some(scheduler_handle);

    Ok(runtime)
}

async fn build_skill_store(
    databases: Arc<crate::storage::AgentDatabases>,
    workspace_root: &Path,
    hub_config: Option<crate::config::HubConfig>,
    sync_interval_secs: u64,
    cancel: CancellationToken,
) -> (Arc<SkillStore>, usize, tokio::task::JoinHandle<()>) {
    let store = Arc::new(SkillStore::new(
        databases,
        workspace_root.to_path_buf(),
        hub_config,
    ));

    if let Err(e) = store.refresh().await {
        tracing::warn!(error = %e, "initial skill sync failed, starting with empty store");
    } else {
        tracing::info!("skills loaded");
    }

    let skill_count = store.loaded_skills().len();

    let sync_handle = crate::kernel::skills::remote::sync::spawn_sync_task(
        store.clone(),
        sync_interval_secs,
        cancel,
    );

    (store, skill_count, sync_handle)
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
