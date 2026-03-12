use std::sync::Arc;

use parking_lot::RwLock;

use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterService;
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
    pub(crate) skills: Arc<SkillStore>,
    pub(crate) sessions: Arc<SessionManager>,
    pub(crate) channels: Arc<ChannelRegistry>,
    pub(crate) supervisor: Arc<ChannelSupervisor>,
    pub(crate) status: RwLock<RuntimeStatus>,
    pub(crate) sync_cancel: tokio_util::sync::CancellationToken,
    pub(crate) sync_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) scheduler_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub(crate) cluster: Option<Arc<ClusterService>>,
    pub(crate) heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
}

pub(crate) struct RuntimeParts {
    pub config: AgentConfig,
    pub databases: Arc<crate::storage::AgentDatabases>,
    pub llm: RwLock<Arc<dyn LLMProvider>>,
    pub skills: Arc<SkillStore>,
    pub sessions: Arc<SessionManager>,
    pub channels: Arc<ChannelRegistry>,
    pub supervisor: Arc<ChannelSupervisor>,
    pub status: RwLock<RuntimeStatus>,
    pub sync_cancel: tokio_util::sync::CancellationToken,
    pub sync_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub scheduler_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
    pub cluster: Option<Arc<ClusterService>>,
    pub heartbeat_handle: RwLock<Option<tokio::task::JoinHandle<()>>>,
}

impl Runtime {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        api_base_url: &str,
        api_token: &str,
        warehouse: &str,
        db_prefix: &str,
        instance_id: &str,
        llm: Arc<dyn LLMProvider>,
    ) -> super::runtime_builder::Builder {
        super::runtime_builder::Builder::new(
            api_base_url,
            api_token,
            warehouse,
            db_prefix,
            instance_id,
            llm,
        )
    }

    pub(crate) fn from_parts(parts: RuntimeParts) -> Self {
        Self {
            config: parts.config,
            databases: parts.databases,
            llm: parts.llm,
            skills: parts.skills,
            sessions: parts.sessions,
            channels: parts.channels,
            supervisor: parts.supervisor,
            status: parts.status,
            sync_cancel: parts.sync_cancel,
            sync_handle: parts.sync_handle,
            scheduler_handle: parts.scheduler_handle,
            cluster: parts.cluster,
            heartbeat_handle: parts.heartbeat_handle,
        }
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
