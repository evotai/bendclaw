use std::collections::HashMap;
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
use crate::kernel::channel::chat_router::ChatHandler;
use crate::kernel::channel::chat_router::ChatRouter;
use crate::kernel::channel::chat_router::ChatRouterConfig;
use crate::kernel::channel::debouncer::DebounceConfig;
use crate::kernel::channel::dispatch::dispatch_debounced;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterOptions;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::agent_config::CheckpointConfig;
use crate::kernel::runtime::diagnostics;
use crate::kernel::runtime::runtime::Runtime;
use crate::kernel::runtime::runtime::RuntimeParts;
use crate::kernel::runtime::runtime::RuntimeStatus;
use crate::kernel::runtime::ActivityTracker;
use crate::kernel::session::SessionLifecycle;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::store::SkillStore;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

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
        let config = AgentConfig {
            node_id: self.node_id,
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

    let sync_cancel = CancellationToken::new();

    let workspace_root = Path::new(&config.workspace.root_dir);
    let (skills, _, sync_handle) = build_skill_store(
        databases.clone(),
        workspace_root,
        hub_config,
        skills_sync_interval_secs,
        sync_cancel.clone(),
    )
    .await;

    let sessions = Arc::new(SessionManager::new());
    let channels = Arc::new(build_channel_registry());
    let activity_tracker = Arc::new(ActivityTracker::new());
    let trace_writer = crate::kernel::trace::TraceWriter::spawn();
    let persist_writer = crate::kernel::run::persist_op::spawn_persist_writer();
    let session_lifecycle = Arc::new(SessionLifecycle::new(
        databases.clone(),
        sessions.clone(),
        persist_writer.clone(),
    ));
    let channel_message_writer = crate::kernel::channel::spawn_channel_message_writer();
    let rate_limiter = Arc::new(
        crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter::new(
            crate::kernel::channel::delivery::rate_limit::RateLimitConfig::default(),
        ),
    );
    let tool_writer = crate::kernel::writer::tool_op::spawn_tool_writer();

    // Directive and cluster start as None — initialized in background after lease.
    let (directive, directive_handle) = (None, None);
    let (cluster_service, heartbeat_handle) = (None, None);

    // Use Arc::new_cyclic so the ChatRouter handler can capture a Weak<Runtime>.
    let runtime = Arc::new_cyclic(|weak: &std::sync::Weak<Runtime>| {
        let weak_for_handler = weak.clone();
        let handler: ChatHandler = Arc::new(move |input| {
            let weak = weak_for_handler.clone();
            Box::pin(async move {
                if let Some(runtime) = weak.upgrade() {
                    dispatch_debounced(&runtime, input).await;
                }
            })
        });
        let chat_router = Arc::new(ChatRouter::new(
            ChatRouterConfig::default(),
            DebounceConfig::default(),
            handler,
        ));
        let supervisor = Arc::new(ChannelSupervisor::new(
            channels.clone(),
            chat_router.clone(),
        ));

        Runtime::from_parts(RuntimeParts {
            sessions,
            session_lifecycle,
            channels,
            supervisor,
            chat_router,
            config,
            databases,
            llm: RwLock::new(llm),
            agent_llms: RwLock::new(HashMap::new()),
            skills,
            status: RwLock::new(RuntimeStatus::Ready),
            sync_cancel: sync_cancel.clone(),
            sync_handle: RwLock::new(Some(sync_handle)),
            lease_handle: RwLock::new(None),
            cluster: RwLock::new(cluster_service),
            heartbeat_handle: RwLock::new(heartbeat_handle),
            directive: RwLock::new(directive),
            directive_handle: RwLock::new(directive_handle),
            activity_tracker,
            trace_writer,
            persist_writer,
            channel_message_writer,
            rate_limiter,
            health_monitor_handle: RwLock::new(None),
            tool_writer,
        })
    });

    let http_client = reqwest::Client::new();
    let mut lease_builder =
        crate::kernel::lease::LeaseServiceBuilder::new(&runtime.config().node_id);
    lease_builder.register(Arc::new(
        crate::kernel::channel::lease::ChannelLeaseResource::new(
            runtime.databases().clone(),
            runtime.channels().clone(),
            runtime.supervisor().clone(),
        ),
    ));
    lease_builder.register(Arc::new(
        crate::kernel::task::lease::TaskLeaseResource::new(runtime.clone(), http_client),
    ));
    let lease_handle = lease_builder.spawn(sync_cancel.clone());
    *runtime.lease_handle.write() = Some(lease_handle);

    // Spawn channel health monitor.
    {
        use crate::kernel::channel::delivery::health::ChannelHealthMonitor;
        use crate::kernel::channel::delivery::health::HealthMonitorConfig;

        let monitor = Arc::new(ChannelHealthMonitor::new(
            runtime.supervisor().clone(),
            HealthMonitorConfig::default(),
        ));
        let handle = monitor.spawn(vec![], sync_cancel.clone());
        *runtime.health_monitor_handle.write() = Some(handle);
    }

    // Initialize directive and cluster in background — lease is already running.
    if let Some(dc) = directive_config {
        let rt = runtime.clone();
        let cancel = sync_cancel.clone();
        crate::base::spawn_fire_and_forget("directive_init", async move {
            let client = match DirectiveClient::new(&dc.api_base, &dc.token) {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    diagnostics::log_runtime_directive_init_failed(&e);
                    return;
                }
            };
            let service = Arc::new(DirectiveService::new(
                client,
                DirectiveService::DEFAULT_REFRESH_INTERVAL,
            ));
            if let Err(e) = service.refresh().await {
                diagnostics::log_runtime_directive_init_failed(&e);
            }
            let handle = service.spawn_refresh_loop(cancel);
            *rt.directive.write() = Some(service);
            *rt.directive_handle.write() = Some(handle);
        });
    }

    if let Some(cc) = cluster_config {
        let rt = runtime.clone();
        let cancel = sync_cancel;
        crate::base::spawn_fire_and_forget("cluster_init", async move {
            let cluster_client = Arc::new(ClusterClient::new(
                &cc.registry_url,
                &cc.registry_token,
                &rt.config().node_id,
                &cc.advertise_url,
                &cc.cluster_id,
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
                diagnostics::log_runtime_cluster_init_failed(&e);
                return;
            }
            let hb_handle = svc.spawn_heartbeat(cancel);
            *rt.cluster.write() = Some(svc);
            *rt.heartbeat_handle.write() = Some(hb_handle);
        });
    }

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

    let skill_count = store.loaded_skills().len();

    // Initial refresh is handled by the background sync task (first tick
    // fires immediately), so the server can start accepting requests without
    // waiting for git clone / DB skill sync.
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
