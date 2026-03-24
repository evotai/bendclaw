use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::ActivityGuard;
use super::ActivityTracker;
use super::SuspendStatus;
use crate::kernel::channel::delivery::outbound_queue::OutboundQueue;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::lease::LeaseServiceHandle;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::turn_coordinator::TurnStateStore;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::store::SkillStore;
use crate::llm::provider::LLMProvider;
use crate::storage::pool::Pool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    Building,
    Ready,
    ShuttingDown,
    Stopped,
}

pub struct Runtime {
    pub(crate) config: AgentConfig,
    pub(crate) databases: Arc<crate::storage::AgentDatabases>,
    pub(crate) llm: RwLock<Arc<dyn LLMProvider>>,
    pub(crate) agent_llms: RwLock<HashMap<String, Arc<dyn LLMProvider>>>,
    pub(crate) skills: Arc<SkillStore>,
    pub(crate) sessions: Arc<SessionManager>,
    pub(crate) channels: Arc<ChannelRegistry>,
    pub(crate) supervisor: Arc<ChannelSupervisor>,
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
    pub(crate) outbound_queue: OutboundQueue,
    pub(crate) rate_limiter: Arc<OutboundRateLimiter>,
    pub(crate) health_monitor_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub(crate) channel_session_keys: RwLock<HashMap<String, String>>,
    pub(crate) turn_states: TurnStateStore,
}

pub struct RuntimeParts {
    pub config: AgentConfig,
    pub databases: Arc<crate::storage::AgentDatabases>,
    pub llm: RwLock<Arc<dyn LLMProvider>>,
    pub agent_llms: RwLock<HashMap<String, Arc<dyn LLMProvider>>>,
    pub skills: Arc<SkillStore>,
    pub sessions: Arc<SessionManager>,
    pub channels: Arc<ChannelRegistry>,
    pub supervisor: Arc<ChannelSupervisor>,
    pub status: RwLock<RuntimeStatus>,
    pub sync_cancel: tokio_util::sync::CancellationToken,
    pub sync_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub lease_handle: RwLock<Option<LeaseServiceHandle>>,
    pub cluster: RwLock<Option<Arc<ClusterService>>>,
    pub heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub directive: RwLock<Option<Arc<DirectiveService>>>,
    pub directive_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub activity_tracker: Arc<ActivityTracker>,
    pub trace_writer: crate::kernel::trace::TraceWriter,
    pub persist_writer: crate::kernel::run::persist_op::PersistWriter,
    pub channel_message_writer: crate::kernel::channel::ChannelMessageWriter,
    pub outbound_queue: OutboundQueue,
    pub rate_limiter: Arc<OutboundRateLimiter>,
    pub health_monitor_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    pub channel_session_keys: RwLock<HashMap<String, String>>,
    pub turn_states: TurnStateStore,
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
            skills: parts.skills,
            sessions: parts.sessions,
            channels: parts.channels,
            supervisor: parts.supervisor,
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
            outbound_queue: parts.outbound_queue,
            rate_limiter: parts.rate_limiter,
            health_monitor_handle: parts.health_monitor_handle,
            tool_writer: parts.tool_writer,
            channel_session_keys: parts.channel_session_keys,
            turn_states: parts.turn_states,
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

    pub fn skill_prompt(&self, agent_id: &str) -> String {
        self.skills
            .for_agent(agent_id)
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
        slog!(debug, "runtime", "reloaded",);
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
            slog!(
                info,
                "runtime",
                "invalidated",
                agent_id,
                evicted_idle = result.evicted_idle,
                marked_running = result.marked_running,
            );
        }
    }

    pub fn skills(&self) -> &Arc<SkillStore> {
        &self.skills
    }

    pub fn sessions(&self) -> &Arc<SessionManager> {
        &self.sessions
    }

    pub fn channels(&self) -> &Arc<ChannelRegistry> {
        &self.channels
    }

    pub fn supervisor(&self) -> &Arc<ChannelSupervisor> {
        &self.supervisor
    }

    pub fn model(&self) -> String {
        self.llm.read().default_model().to_string()
    }

    pub fn temperature(&self) -> f64 {
        self.llm.read().default_temperature()
    }

    // ── Channel session key resolution ──

    /// Resolve the current session key for a channel base key.
    /// Checks in-memory cache first, then DB for the latest session with this prefix.
    /// Falls back to the base key itself (backward compatible).
    pub async fn resolve_channel_session_key(&self, base_key: &str, agent_id: &str) -> String {
        // 1. Check in-memory cache
        if let Some(key) = self.channel_session_keys.read().get(base_key) {
            return key.clone();
        }
        // 2. Query DB for latest session with this prefix
        if let Ok(pool) = self.databases.agent_pool(agent_id) {
            let repo = crate::storage::dal::session::repo::SessionRepo::new(pool);
            if let Ok(Some(record)) = repo.latest_by_prefix(base_key).await {
                self.channel_session_keys
                    .write()
                    .insert(base_key.to_string(), record.id.clone());
                return record.id;
            }
        }
        // 3. Fallback: use base key (no prior session exists)
        base_key.to_string()
    }

    /// Rotate to a new session key for a channel base key.
    /// Returns the new session key with `#timestamp` suffix.
    pub fn rotate_channel_session_key(&self, base_key: &str) -> String {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let new_key = format!("{base_key}#{ts}");
        self.channel_session_keys
            .write()
            .insert(base_key.to_string(), new_key.clone());
        new_key
    }
}

pub use crate::kernel::validate_agent_id;
use crate::observability::log::slog;
