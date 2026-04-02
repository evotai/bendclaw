use super::agent_repo::AgentRepo;
use super::channel_repo::ChannelRepo;
use super::kind::StorageKind;
use super::run_event_repo::RunEventRepo;
use super::run_repo::RunRepo;
use super::session_repo::SessionRepo;
use super::skill_repo::SkillRepo;
use super::span_repo::SpanRepo;
use super::task_history_repo::TaskHistoryRepo;
use super::task_repo::TaskRepo;
use super::trace_repo::TraceRepo;

/// Full storage backend — startup-only.
///
/// Startup constructs one implementation (LocalFsBackend or DatabendBackend),
/// then projects it into narrow repo trait objects for injection into kernel
/// modules. No kernel module ever sees this trait directly.
pub trait StorageBackend: Send + Sync {
    fn kind(&self) -> StorageKind;

    fn agent_repo(&self) -> &dyn AgentRepo;
    fn skill_repo(&self) -> &dyn SkillRepo;
    fn channel_repo(&self) -> &dyn ChannelRepo;
    fn session_repo(&self) -> &dyn SessionRepo;
    fn run_repo(&self) -> &dyn RunRepo;
    fn run_event_repo(&self) -> &dyn RunEventRepo;
    fn trace_repo(&self) -> &dyn TraceRepo;
    fn span_repo(&self) -> &dyn SpanRepo;
    fn task_repo(&self) -> &dyn TaskRepo;
    fn task_history_repo(&self) -> &dyn TaskHistoryRepo;
}
