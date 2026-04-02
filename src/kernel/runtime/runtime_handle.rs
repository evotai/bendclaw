use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::runtime_state::RuntimeParts;
use super::runtime_state::RuntimeStatus;
use super::ActivityGuard;
use super::ActivityTracker;
use super::SuspendStatus;
use crate::kernel::channel::chat_router::ChatRouter;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::lease::LeaseServiceHandle;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::diagnostics;
use crate::kernel::runtime::org::OrgServices;
use crate::kernel::session::store::lifecycle::SessionLifecycle;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

pub struct Runtime {
    pub(crate) config: AgentConfig,
    pub(crate) databases: Arc<crate::storage::AgentDatabases>,
    pub(crate) llm: RwLock<Arc<dyn LLMProvider>>,
    pub(crate) agent_llms: RwLock<HashMap<String, Arc<dyn LLMProvider>>>,
    pub(crate) org: Arc<OrgServices>,
    pub(crate) catalog: Arc<SkillCatalog>,
    pub(crate) sessions: Arc<SessionManager>,
    pub(crate) session_lifecycle: Arc<SessionLifecycle>,
    pub(crate) channels: Arc<ChannelRegistry>,
    pub(crate) supervisor: Arc<ChannelSupervisor>,
    pub(crate) chat_router: Arc<ChatRouter>,
    pub(crate) status: RwLock<RuntimeStatus>,
    pub(crate) sync_cancel: tokio_util::sync::CancellationToken,
    pub(crate) sync_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) lease_handle: RwLock<Option<LeaseServiceHandle>>,
    pub(crate) cluster: RwLock<Option<Arc<ClusterService>>>,
    pub(crate) heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) directive: RwLock<Option<Arc<DirectiveService>>>,
    pub(crate) directive_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) activity_tracker: Arc<ActivityTracker>,
    pub(crate) trace_writer: crate::kernel::trace::TraceWriter,
    pub(crate) persist_writer: crate::kernel::run::persist_op::PersistWriter,
    pub(crate) channel_message_writer: crate::kernel::channel::ChannelMessageWriter,
    pub(crate) rate_limiter: Arc<OutboundRateLimiter>,
    pub(crate) tool_writer: crate::kernel::writer::tool_op::ToolWriter,
}

impl Runtime {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        api_base_url: &str,
        api_token: &str,
        warehouse: &str,
        db_prefix: &str,
        node_id: &str,
        llm: Arc<dyn LLMProvider>,
    ) -> super::runtime_builder::Builder {
        super::runtime_builder::Builder::new(
            api_base_url,
            api_token,
            warehouse,
            db_prefix,
            node_id,
            llm,
        )
    }

    pub fn from_parts(parts: RuntimeParts) -> Self {
        Self {
            config: parts.config,
            databases: parts.databases,
            llm: parts.llm,
            agent_llms: parts.agent_llms,
            org: parts.org,
            catalog: parts.catalog,
            sessions: parts.sessions,
            session_lifecycle: parts.session_lifecycle,
            channels: parts.channels,
            supervisor: parts.supervisor,
            chat_router: parts.chat_router,
            status: parts.status,
            sync_cancel: parts.sync_cancel,
            sync_handle: parts.sync_handle,
            lease_handle: parts.lease_handle,
            cluster: parts.cluster,
            heartbeat_handle: parts.heartbeat_handle,
            directive: parts.directive,
            directive_handle: parts.directive_handle,
            activity_tracker: parts.activity_tracker,
            trace_writer: parts.trace_writer,
            persist_writer: parts.persist_writer,
            channel_message_writer: parts.channel_message_writer,
            rate_limiter: parts.rate_limiter,
            tool_writer: parts.tool_writer,
        }
    }

    pub fn suspend_status(&self) -> SuspendStatus {
        let active_sessions = self.sessions.active_count();
        let active_tasks = self.activity_tracker.active_task_count();
        let active_leases = self
            .lease_handle
            .read()
            .as_ref()
            .map(|h| h.active_lease_count())
            .unwrap_or(0);
        SuspendStatus {
            can_suspend: active_sessions == 0 && active_tasks == 0 && active_leases == 0,
            active_sessions,
            active_tasks,
            active_leases,
        }
    }

    pub fn track_task(&self) -> ActivityGuard {
        self.activity_tracker.track_task()
    }

    pub fn skill_prompt(&self, user_id: &str) -> String {
        self.catalog
            .visible_skills(user_id)
            .into_iter()
            .filter(|s| !s.executable)
            .map(|s| s.content)
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    pub fn database(&self) -> &Pool {
        self.databases.root_pool()
    }

    pub fn databases(&self) -> &Arc<crate::storage::AgentDatabases> {
        &self.databases
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    /// Wait until all queued persist operations have been processed.
    /// Used by the non-stream path to ensure DB writes complete before
    /// reading back the run record.
    pub async fn flush_persist(&self) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.persist_writer
            .send(crate::kernel::run::persist_op::PersistOp::Flush(tx));
        let _ = rx.await;
    }

    pub fn llm(&self) -> Arc<dyn LLMProvider> {
        self.llm.read().clone()
    }

    pub fn reload_llm(&self, new_llm: Arc<dyn LLMProvider>) {
        *self.llm.write() = new_llm;
    }

    /// Resolve the LLM provider for a specific agent.
    /// Checks the per-agent cache first, then queries DB for agent-specific config,
    /// and falls back to the global LLM provider.
    pub async fn resolve_agent_llm(
        &self,
        agent_id: &str,
        pool: &Pool,
    ) -> crate::base::Result<Arc<dyn LLMProvider>> {
        let (llm, _config) = self.resolve_agent_llm_and_config(agent_id, pool).await?;
        Ok(llm)
    }

    /// Resolve LLM provider and return the agent config record (if any).
    /// Single DB query shared by session factory for both LLM resolution
    /// and config caching.
    pub async fn resolve_agent_llm_and_config(
        &self,
        agent_id: &str,
        pool: &Pool,
    ) -> crate::base::Result<(
        Arc<dyn LLMProvider>,
        Option<crate::storage::dal::agent_config::record::AgentConfigRecord>,
    )> {
        // Check LLM cache (still need to fetch config for caller)
        let cached_llm = self.agent_llms.read().get(agent_id).cloned();

        let config_store =
            crate::storage::dal::agent_config::repo::AgentConfigStore::new(pool.clone());
        let config_record = config_store.get(agent_id).await?;

        let llm = if let Some(provider) = cached_llm {
            provider
        } else if let Some(ref record) = config_record {
            if let Some(ref llm_config) = record.llm_config {
                let router = crate::llm::router::LLMRouter::from_config(llm_config)?;
                let provider: Arc<dyn LLMProvider> = Arc::new(router);
                self.agent_llms
                    .write()
                    .insert(agent_id.to_string(), provider.clone());
                provider
            } else {
                self.llm.read().clone()
            }
        } else {
            self.llm.read().clone()
        };

        Ok((llm, config_record))
    }

    /// Remove a cached per-agent LLM provider, forcing re-resolution on next use.
    /// Also marks live sessions stale and evicts the idle ones so future turns
    /// pick up the new config.
    pub fn invalidate_agent_llm(&self, agent_id: &str) {
        self.agent_llms.write().remove(agent_id);
        let result = self.sessions.invalidate_by_agent(agent_id);
        if result.evicted_idle > 0 || result.marked_running > 0 {
            diagnostics::log_runtime_invalidated(
                agent_id,
                result.evicted_idle,
                result.marked_running,
            );
        }
    }

    pub fn skills(&self) -> &Arc<SkillCatalog> {
        &self.catalog
    }

    pub fn org(&self) -> &Arc<OrgServices> {
        &self.org
    }

    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.sessions
    }

    pub fn session_lifecycle(&self) -> &Arc<SessionLifecycle> {
        &self.session_lifecycle
    }

    pub fn channels(&self) -> &Arc<ChannelRegistry> {
        &self.channels
    }

    pub fn supervisor(&self) -> &Arc<ChannelSupervisor> {
        &self.supervisor
    }

    pub fn chat_router(&self) -> &Arc<ChatRouter> {
        &self.chat_router
    }

    pub fn model(&self) -> String {
        self.llm.read().default_model().to_string()
    }

    pub fn temperature(&self) -> f64 {
        self.llm.read().default_temperature()
    }
}
