use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::client::BendclawClient;
use crate::client::ClusterClient;
use crate::client::DirectiveClient;
use crate::cluster::ClusterOptions;
use crate::cluster::ClusterService;
use crate::config::agent::AgentConfig;
use crate::config::ClusterConfig;
use crate::config::DirectiveConfig;
use crate::directive::DirectiveService;
use crate::kernel::runtime::diagnostics;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::runtime::runtime::Runtime;
use crate::kernel::runtime::runtime_parts::RuntimeParts;
use crate::kernel::runtime::runtime_parts::RuntimeStatus;
use crate::kernel::runtime::runtime_services;
use crate::kernel::runtime::ActivityTracker;
use crate::llm::provider::LLMProvider;
use crate::sessions::store::lifecycle::SessionLifecycle;
use crate::sessions::SessionManager;
use crate::skills::sync::SkillIndex;
use crate::storage::pool::Pool;
use crate::subscriptions::SharedSubscriptionStore;
use crate::subscriptions::SubscriptionStore;
use crate::types::Result;

pub(super) struct RuntimeDeps {
    pub config: AgentConfig,
    pub llm: Arc<dyn LLMProvider>,
    pub databases: Arc<crate::storage::AgentDatabases>,
    pub org: Arc<OrgServices>,
    pub catalog: Arc<SkillIndex>,
    pub sync_cancel: CancellationToken,
}

pub(super) fn assemble_runtime(deps: RuntimeDeps) -> Arc<Runtime> {
    let sessions = Arc::new(SessionManager::new());
    let channels = Arc::new(runtime_services::build_channel_registry());
    let activity_tracker = Arc::new(ActivityTracker::new());
    let writers = runtime_services::spawn_writers();
    let session_lifecycle = Arc::new(SessionLifecycle::new(
        deps.databases.clone(),
        sessions.clone(),
        writers.persist_writer.clone(),
    ));
    let sync_cancel = deps.sync_cancel;

    Arc::new_cyclic(|weak: &std::sync::Weak<Runtime>| {
        let chat_router = runtime_services::build_chat_router(weak);
        let supervisor = runtime_services::build_supervisor(channels.clone(), chat_router.clone());

        Runtime::from_parts(RuntimeParts {
            sessions,
            session_lifecycle,
            channels,
            supervisor,
            chat_router,
            config: deps.config,
            databases: deps.databases,
            llm: RwLock::new(deps.llm),
            agent_llms: RwLock::new(HashMap::new()),
            org: deps.org,
            catalog: deps.catalog,
            status: RwLock::new(RuntimeStatus::Ready),
            sync_cancel,
            sync_handle: RwLock::new(None),
            lease_handle: RwLock::new(None),
            cluster: RwLock::new(None),
            heartbeat_handle: RwLock::new(None),
            directive: RwLock::new(None),
            directive_handle: RwLock::new(None),
            activity_tracker,
            trace_writer: writers.trace_writer,
            persist_writer: writers.persist_writer,
            channel_message_writer: writers.channel_message_writer,
            rate_limiter: writers.rate_limiter,
            tool_writer: writers.tool_writer,
        })
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn construct(
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
    let skill_store_for_catalog = Arc::new(crate::skills::store::DatabendSharedSkillStore::new(
        pool.with_database("evotai_meta")?,
    ));
    let sub_store: Arc<dyn SubscriptionStore> = Arc::new(SharedSubscriptionStore::new(
        pool.with_database("evotai_meta")?,
    ));
    let catalog = Arc::new(SkillIndex::new(
        workspace_root.to_path_buf(),
        skill_store_for_catalog,
        sub_store,
        hub_config,
    ));

    let meta_pool = pool.with_database("evotai_meta")?;
    crate::storage::migrator::run_org(&meta_pool).await;
    let org = Arc::new(OrgServices::new(
        meta_pool,
        catalog.clone(),
        &config,
        llm.clone(),
    ));

    let runtime = assemble_runtime(RuntimeDeps {
        config,
        llm,
        databases,
        org,
        catalog: catalog.clone(),
        sync_cancel: sync_cancel.clone(),
    });

    let http_client = reqwest::Client::new();
    let mut lease_builder = crate::lease::LeaseServiceBuilder::new(&runtime.config().node_id);
    lease_builder.register(Arc::new(
        crate::channels::model::lease::ChannelLeaseResource::new(
            runtime.databases().clone(),
            runtime.channels().clone(),
            runtime.supervisor().clone(),
        ),
    ));
    lease_builder.register(Arc::new(crate::tasks::lease::TaskLeaseResource::new(
        runtime.clone(),
        http_client,
    )));
    let lease_handle = lease_builder.spawn(sync_cancel.clone());
    *runtime.lease_handle.write() = Some(lease_handle);

    if let Some(dc) = directive_config {
        let rt = runtime.clone();
        let cancel = sync_cancel.clone();
        crate::types::spawn_fire_and_forget("directive_init", async move {
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
        crate::types::spawn_fire_and_forget("cluster_init", async move {
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

    {
        let sync_catalog = runtime.catalog.clone();
        let sync_databases = runtime.databases().clone();
        let cancel = runtime.sync_cancel.clone();
        let sync_handle = crate::types::spawn_named("skill_sync_loop", async move {
            let base_interval = std::time::Duration::from_secs(skills_sync_interval_secs);
            let mut consecutive_errors: u64 = 0;
            let mut next_sleep = std::time::Duration::ZERO;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => { break; }
                    _ = tokio::time::sleep(next_sleep) => {
                        sync_catalog.ensure_hub();
                        let user_ids = match sync_databases.list_user_ids().await {
                            Ok(ids) => ids,
                            Err(e) => {
                                crate::skills::diagnostics::log_skill_sync_failed(&e, consecutive_errors + 1);
                                consecutive_errors += 1;
                                let secs = (60u64 << (consecutive_errors - 1).min(3)).min(300);
                                next_sleep = std::time::Duration::from_secs(secs);
                                continue;
                            }
                        };
                        let mut had_error = false;
                        for user_id in &user_ids {
                            if let Err(e) = sync_catalog.reconcile(user_id).await {
                                if !had_error {
                                    consecutive_errors += 1;
                                    had_error = true;
                                }
                                if consecutive_errors == 1 || consecutive_errors.is_multiple_of(20) {
                                    crate::skills::diagnostics::log_skill_sync_failed(&e, consecutive_errors);
                                }
                            }
                        }
                        if had_error {
                            let secs = (60u64 << (consecutive_errors - 1).min(3)).min(300);
                            next_sleep = std::time::Duration::from_secs(secs);
                        } else {
                            consecutive_errors = 0;
                            next_sleep = base_interval;
                        }
                    }
                }
            }
        });
        *runtime.sync_handle.write() = Some(sync_handle);
    }

    Ok(runtime)
}

pub(super) async fn construct_minimal(
    config: AgentConfig,
    llm: Arc<dyn LLMProvider>,
    root_pool: Option<Pool>,
) -> Result<Arc<Runtime>> {
    let pool = match root_pool {
        Some(pool) => pool,
        None => Pool::noop(),
    };
    let databases = Arc::new(crate::storage::AgentDatabases::new(
        pool.clone(),
        &config.db_prefix,
    )?);

    let workspace_root = Path::new(&config.workspace.root_dir);
    let skill_store = Arc::new(crate::skills::store::DatabendSharedSkillStore::noop());
    let sub_store: Arc<dyn SubscriptionStore> = Arc::new(SharedSubscriptionStore::noop());
    let catalog = Arc::new(SkillIndex::new(
        workspace_root.to_path_buf(),
        skill_store,
        sub_store,
        None,
    ));

    let org = Arc::new(OrgServices::new(
        pool.clone(),
        catalog.clone(),
        &config,
        llm.clone(),
    ));

    let runtime = assemble_runtime(RuntimeDeps {
        config,
        llm,
        databases,
        org,
        catalog,
        sync_cancel: CancellationToken::new(),
    });

    Ok(runtime)
}
