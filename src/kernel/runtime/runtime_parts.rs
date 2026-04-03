use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::channels::egress::rate_limit::OutboundRateLimiter;
use crate::channels::routing::chat_router::ChatRouter;
use crate::channels::runtime::channel_registry::ChannelRegistry;
use crate::channels::runtime::supervisor::ChannelSupervisor;
use crate::config::agent::AgentConfig;
use crate::kernel::cluster::ClusterService;
use crate::kernel::directive::DirectiveService;
use crate::kernel::lease::LeaseServiceHandle;
use crate::kernel::runtime::ActivityTracker;
use crate::llm::provider::LLMProvider;
use crate::sessions::store::lifecycle::SessionLifecycle;
use crate::sessions::SessionManager;
use crate::skills::sync::SkillIndex;

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
    pub catalog: Arc<SkillIndex>,
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
    pub trace_writer: crate::traces::TraceWriter,
    pub persist_writer: crate::execution::persist::persist_op::PersistWriter,
    pub channel_message_writer: crate::channels::ChannelMessageWriter,
    pub rate_limiter: Arc<OutboundRateLimiter>,
    pub tool_writer: crate::kernel::writer::tool_op::ToolWriter,
}
