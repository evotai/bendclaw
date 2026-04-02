use async_trait::async_trait;

use super::agent_repo::AgentRepo;
use super::channel_repo::ChannelRepo;
use super::kind::StorageKind;
use super::run_event_repo::RunEventRepo as RunEventRepoTrait;
use super::run_repo::RunRepo as RunRepoTrait;
use super::session_repo::SessionRepo as SessionRepoTrait;
use super::skill_repo::SkillRepo;
use super::span_repo::SpanRepo;
use super::storage_backend::StorageBackend;
use super::task_history_repo::TaskHistoryRepo;
use super::task_repo::TaskRepo;
use super::trace_repo::TraceRepo;
use crate::base::entities::*;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::pool::Pool;

/// Databend cloud storage backend — adapts existing DAL repos to the
/// unified entity model. Full implementation deferred to Phase 5;
/// this stub satisfies the `StorageBackend` trait contract.
pub struct DatabendBackend {
    _pool: Pool,
}

impl DatabendBackend {
    pub fn new(pool: Pool) -> Self {
        Self { _pool: pool }
    }
}

impl StorageBackend for DatabendBackend {
    fn kind(&self) -> StorageKind {
        StorageKind::Cloud
    }

    fn agent_repo(&self) -> &dyn AgentRepo {
        self
    }
    fn skill_repo(&self) -> &dyn SkillRepo {
        self
    }
    fn channel_repo(&self) -> &dyn ChannelRepo {
        self
    }
    fn session_repo(&self) -> &dyn SessionRepoTrait {
        self
    }
    fn run_repo(&self) -> &dyn RunRepoTrait {
        self
    }
    fn run_event_repo(&self) -> &dyn RunEventRepoTrait {
        self
    }
    fn trace_repo(&self) -> &dyn TraceRepo {
        self
    }
    fn span_repo(&self) -> &dyn SpanRepo {
        self
    }
    fn task_repo(&self) -> &dyn TaskRepo {
        self
    }
    fn task_history_repo(&self) -> &dyn TaskHistoryRepo {
        self
    }
}

fn stub_err(op: &str) -> crate::base::ErrorCode {
    ErrorCode::internal(format!("DatabendBackend::{op} — cloud mapping pending"))
}

// ── Stub implementations ──────────────────────────────────────────────────────

#[async_trait]
impl AgentRepo for DatabendBackend {
    async fn get_agent(&self, _: &str, _: &str) -> Result<Option<Agent>> {
        Err(stub_err("get_agent"))
    }
    async fn save_agent(&self, _: &Agent) -> Result<()> {
        Err(stub_err("save_agent"))
    }
    async fn delete_agent(&self, _: &str, _: &str) -> Result<()> {
        Err(stub_err("delete_agent"))
    }
    async fn list_agents(&self, _: &str) -> Result<Vec<Agent>> {
        Err(stub_err("list_agents"))
    }
}

#[async_trait]
impl SkillRepo for DatabendBackend {
    async fn get_skill(&self, _: &str, _: &str, _: &str) -> Result<Option<Skill>> {
        Err(stub_err("get_skill"))
    }
    async fn save_skill(&self, _: &Skill) -> Result<()> {
        Err(stub_err("save_skill"))
    }
    async fn delete_skill(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Err(stub_err("delete_skill"))
    }
    async fn list_skills(&self, _: &str, _: &str) -> Result<Vec<Skill>> {
        Err(stub_err("list_skills"))
    }
}

#[async_trait]
impl ChannelRepo for DatabendBackend {
    async fn get_channel(&self, _: &str, _: &str, _: &str) -> Result<Option<Channel>> {
        Err(stub_err("get_channel"))
    }
    async fn save_channel(&self, _: &Channel) -> Result<()> {
        Err(stub_err("save_channel"))
    }
    async fn delete_channel(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Err(stub_err("delete_channel"))
    }
    async fn list_channels(&self, _: &str, _: &str) -> Result<Vec<Channel>> {
        Err(stub_err("list_channels"))
    }
}

#[async_trait]
impl SessionRepoTrait for DatabendBackend {
    async fn find_session(&self, _: &str, _: &str, _: &str) -> Result<Option<Session>> {
        Err(stub_err("find_session"))
    }
    async fn find_latest_session(&self, _: &str, _: &str) -> Result<Option<Session>> {
        Err(stub_err("find_latest_session"))
    }
    async fn create_session(&self, _: &Session) -> Result<()> {
        Err(stub_err("create_session"))
    }
    async fn update_session(&self, _: &Session) -> Result<()> {
        Err(stub_err("update_session"))
    }
    async fn list_sessions(&self, _: &str, _: &str) -> Result<Vec<Session>> {
        Err(stub_err("list_sessions"))
    }
}

#[async_trait]
impl RunRepoTrait for DatabendBackend {
    async fn get_run(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Option<Run>> {
        Err(stub_err("get_run"))
    }
    async fn save_run(&self, _: &Run) -> Result<()> {
        Err(stub_err("save_run"))
    }
    async fn list_runs_by_session(&self, _: &str, _: &str, _: &str) -> Result<Vec<Run>> {
        Err(stub_err("list_runs_by_session"))
    }
    async fn load_handoff(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Option<serde_json::Value>> {
        Err(stub_err("load_handoff"))
    }
    async fn save_handoff(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
        _: &serde_json::Value,
    ) -> Result<()> {
        Err(stub_err("save_handoff"))
    }
    async fn clear_handoff(&self, _: &str, _: &str, _: &str, _: &str) -> Result<()> {
        Err(stub_err("clear_handoff"))
    }
    async fn list_incomplete_runs(&self, _: &str, _: &str) -> Result<Vec<Run>> {
        Err(stub_err("list_incomplete_runs"))
    }
}

#[async_trait]
impl RunEventRepoTrait for DatabendBackend {
    async fn append_event(&self, _: &RunEvent) -> Result<()> {
        Err(stub_err("append_event"))
    }
    async fn list_events_by_run(
        &self,
        _: &str,
        _: &str,
        _: &str,
        _: &str,
    ) -> Result<Vec<RunEvent>> {
        Err(stub_err("list_events_by_run"))
    }
}

#[async_trait]
impl TraceRepo for DatabendBackend {
    async fn get_trace(&self, _: &str, _: &str, _: &str) -> Result<Option<Trace>> {
        Err(stub_err("get_trace"))
    }
    async fn save_trace(&self, _: &Trace) -> Result<()> {
        Err(stub_err("save_trace"))
    }
    async fn list_traces_by_run(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Vec<Trace>> {
        Err(stub_err("list_traces_by_run"))
    }
    async fn list_traces_by_session(&self, _: &str, _: &str, _: &str) -> Result<Vec<Trace>> {
        Err(stub_err("list_traces_by_session"))
    }
}

#[async_trait]
impl SpanRepo for DatabendBackend {
    async fn append_span(&self, _: &Span) -> Result<()> {
        Err(stub_err("append_span"))
    }
    async fn list_spans_by_trace(&self, _: &str, _: &str, _: &str) -> Result<Vec<Span>> {
        Err(stub_err("list_spans_by_trace"))
    }
}

#[async_trait]
impl TaskRepo for DatabendBackend {
    async fn get_task(&self, _: &str, _: &str, _: &str) -> Result<Option<Task>> {
        Err(stub_err("get_task"))
    }
    async fn save_task(&self, _: &Task) -> Result<()> {
        Err(stub_err("save_task"))
    }
    async fn delete_task(&self, _: &str, _: &str, _: &str) -> Result<()> {
        Err(stub_err("delete_task"))
    }
    async fn list_tasks(&self, _: &str, _: &str) -> Result<Vec<Task>> {
        Err(stub_err("list_tasks"))
    }
    async fn update_task(&self, _: &Task) -> Result<()> {
        Err(stub_err("update_task"))
    }
}

#[async_trait]
impl TaskHistoryRepo for DatabendBackend {
    async fn append_history(&self, _: &TaskHistory) -> Result<()> {
        Err(stub_err("append_history"))
    }
    async fn list_history_by_task(&self, _: &str, _: &str, _: &str) -> Result<Vec<TaskHistory>> {
        Err(stub_err("list_history_by_task"))
    }
}
