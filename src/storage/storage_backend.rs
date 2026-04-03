use crate::storage::agents::AgentRepo;
use crate::storage::channels::ChannelRepo;
use crate::storage::kind::StorageKind;
use crate::storage::run_events::RunEventRepo;
use crate::storage::runs::RunRepo;
use crate::storage::sessions::SessionRepo;
use crate::storage::skills::SkillRepo;
use crate::storage::task_history::TaskHistoryRepo;
use crate::storage::tasks::TaskRepo;
use crate::storage::traces::SpanRepo;
use crate::storage::traces::TraceRepo;

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
