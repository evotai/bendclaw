use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::kernel::channel::chat_router::ChatRouter;
use crate::kernel::channel::delivery::rate_limit::OutboundRateLimiter;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::lease::LeaseServiceHandle;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::ActivityTracker;
use crate::kernel::session::store::lifecycle::SessionLifecycle;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::catalog::SkillCatalog;
use crate::llm::provider::LLMProvider;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeStatus {
    Building,
    Ready,
    ShuttingDown,
    Stopped,
}

pub struct RuntimeParts {
    pub config: AgentConfig,
    pub databases: Arc<crate::storage::AgentDatabases>,
    pub llm: RwLock<Arc<dyn LLMProvider>>,
    pub agent_llms: RwLock<HashMap<String, Arc<dyn LLMProvider>>>,
    pub org: Arc<super::org::OrgServices>,
    pub catalog: Arc<SkillCatalog>,
    pub sessions: Arc<SessionManager>,
    pub session_lifecycle: Arc<SessionLifecycle>,
    pub channels: Arc<ChannelRegistry>,
    pub supervisor: Arc<ChannelSupervisor>,
    pub chat_router: Arc<ChatRouter>,
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
    pub rate_limiter: Arc<OutboundRateLimiter>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
}
