use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use super::ActivityGuard;
use super::ActivityTracker;
use super::SuspendStatus;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::lease::LeaseServiceHandle;
use crate::kernel::runtime::agent_config::AgentConfig;
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
    pub(crate) cluster: Option<Arc<ClusterService>>,
    pub(crate) heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) directive: Option<Arc<DirectiveService>>,
    pub(crate) directive_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) activity_tracker: Arc<ActivityTracker>,
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
    pub cluster: Option<Arc<ClusterService>>,
    pub heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub directive: Option<Arc<DirectiveService>>,
    pub directive_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub activity_tracker: Arc<ActivityTracker>,
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

    pub fn llm(&self) -> Arc<dyn LLMProvider> {
        self.llm.read().clone()
    }

    pub fn reload_llm(&self, new_llm: Arc<dyn LLMProvider>) {
        *self.llm.write() = new_llm;
        tracing::info!("LLM provider hot-reloaded");
    }

    /// Resolve the LLM provider for a specific agent.
    /// Checks the per-agent cache first, then queries DB for agent-specific config,
    /// and falls back to the global LLM provider.
    pub async fn resolve_agent_llm(
        &self,
        agent_id: &str,
        pool: &Pool,
    ) -> crate::base::Result<Arc<dyn LLMProvider>> {
        // Check cache
        if let Some(provider) = self.agent_llms.read().get(agent_id) {
            return Ok(provider.clone());
        }

        // Query DB for agent-specific LLM config
        let config_store =
            crate::storage::dal::agent_config::repo::AgentConfigStore::new(pool.clone());
        if let Some(record) = config_store.get(agent_id).await? {
            if let Some(llm_config) = record.llm_config {
                let router = crate::llm::router::LLMRouter::from_config(&llm_config)?;
                let provider: Arc<dyn LLMProvider> = Arc::new(router);
                self.agent_llms
                    .write()
                    .insert(agent_id.to_string(), provider.clone());
                return Ok(provider);
            }
        }

        // Fallback to global
        Ok(self.llm.read().clone())
    }

    /// Remove a cached per-agent LLM provider, forcing re-resolution on next use.
    /// Also marks live sessions stale and evicts the idle ones so future turns
    /// pick up the new config.
    pub fn invalidate_agent_llm(&self, agent_id: &str) {
        self.agent_llms.write().remove(agent_id);
        let result = self.sessions.invalidate_by_agent(agent_id);
        if result.evicted_idle > 0 || result.marked_running > 0 {
            tracing::info!(
                agent_id,
                evicted_idle = result.evicted_idle,
                marked_running = result.marked_running,
                "invalidated sessions after LLM config change"
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

    pub fn temperature(&self) -> f32 {
        self.llm.read().default_temperature()
    }
}

pub use crate::kernel::validate_agent_id;
