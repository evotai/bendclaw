use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;

use crate::storage::agents::AgentRepo;
use crate::storage::channels::ChannelRepo;
use crate::storage::kind::StorageKind;
use crate::storage::run_events::RunEventRepo;
use crate::storage::runs::RunRepo;
use crate::storage::sessions::SessionRepo;
use crate::storage::skills::SkillRepo;
use crate::storage::storage_backend::StorageBackend;
use crate::storage::task_history::TaskHistoryRepo;
use crate::storage::tasks::TaskRepo;
use crate::storage::traces::SpanRepo;
use crate::storage::traces::TraceRepo;
use crate::types::entities::*;
use crate::types::ErrorCode;
use crate::types::Result;

/// Local-first filesystem storage backend.
///
/// Layout:
/// ```text
/// <root>/users/<user_id>/agents/<agent_id>/
///   agent.json
///   skills/<skill_id>.json
///   channels/<channel_id>.json
///   tasks/<task_id>.json
///   task-history/<task_id>.jsonl
///   sessions/<session_id>.json
///   runs/<run_id>.json
///   run-events/<run_id>.jsonl
///   traces/<trace_id>.json
///   spans/<trace_id>.jsonl
/// ```
pub struct LocalFsBackend {
    root: PathBuf,
}

impl LocalFsBackend {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn default_root() -> PathBuf {
        if let Ok(home) = std::env::var("HOME") {
            PathBuf::from(home).join(".bendclaw")
        } else if let Ok(profile) = std::env::var("USERPROFILE") {
            PathBuf::from(profile).join(".bendclaw")
        } else {
            PathBuf::from(".bendclaw")
        }
    }

    fn agent_dir(&self, user_id: &str, agent_id: &str) -> PathBuf {
        self.root
            .join("users")
            .join(user_id)
            .join("agents")
            .join(agent_id)
    }
}

impl StorageBackend for LocalFsBackend {
    fn kind(&self) -> StorageKind {
        StorageKind::Local
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
    fn session_repo(&self) -> &dyn SessionRepo {
        self
    }
    fn run_repo(&self) -> &dyn RunRepo {
        self
    }
    fn run_event_repo(&self) -> &dyn RunEventRepo {
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

// ── Filesystem helpers ────────────────────────────────────────────────────────

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match std::fs::read_to_string(path) {
        Ok(data) => {
            let val = serde_json::from_str(&data)
                .map_err(|e| ErrorCode::internal(format!("parse {}: {e}", path.display())))?;
            Ok(Some(val))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ErrorCode::internal(format!("read {}: {e}", path.display()))),
    }
}

fn write_json<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ErrorCode::internal(format!("mkdir {}: {e}", parent.display())))?;
    }
    let data = serde_json::to_string_pretty(val)
        .map_err(|e| ErrorCode::internal(format!("serialize {}: {e}", path.display())))?;
    std::fs::write(path, data)
        .map_err(|e| ErrorCode::internal(format!("write {}: {e}", path.display())))
}

fn delete_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ErrorCode::internal(format!(
            "delete {}: {e}",
            path.display()
        ))),
    }
}

fn list_json_dir<T: serde::de::DeserializeOwned>(dir: &Path) -> Result<Vec<T>> {
    list_json_dir_filtered(dir, |name| {
        name.ends_with(".json") && !name.contains(".handoff.")
    })
}

fn list_json_dir_filtered<T: serde::de::DeserializeOwned>(
    dir: &Path,
    filter: impl Fn(&str) -> bool,
) -> Result<Vec<T>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => {
            return Err(ErrorCode::internal(format!(
                "readdir {}: {e}",
                dir.display()
            )));
        }
    };
    let mut items = Vec::new();
    for entry in entries {
        let entry = entry
            .map_err(|e| ErrorCode::internal(format!("readdir entry {}: {e}", dir.display())))?;
        let path = entry.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if filter(name) {
            if let Some(item) = read_json::<T>(&path)? {
                items.push(item);
            }
        }
    }
    Ok(items)
}

fn append_jsonl<T: serde::Serialize>(path: &Path, val: &T) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| ErrorCode::internal(format!("mkdir {}: {e}", parent.display())))?;
    }
    let line = serde_json::to_string(val)
        .map_err(|e| ErrorCode::internal(format!("serialize {}: {e}", path.display())))?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| ErrorCode::internal(format!("open {}: {e}", path.display())))?;
    writeln!(file, "{line}")
        .map_err(|e| ErrorCode::internal(format!("write {}: {e}", path.display())))
}

fn read_jsonl<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let data = match std::fs::read_to_string(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => {
            return Err(ErrorCode::internal(format!("read {}: {e}", path.display())));
        }
    };
    let mut items = Vec::new();
    for (i, line) in data.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let item = serde_json::from_str(line)
            .map_err(|e| ErrorCode::internal(format!("parse {}:{}: {e}", path.display(), i + 1)))?;
        items.push(item);
    }
    Ok(items)
}

// ── AgentRepo ─────────────────────────────────────────────────────────────────

#[async_trait]
impl AgentRepo for LocalFsBackend {
    async fn get_agent(&self, user_id: &str, agent_id: &str) -> Result<Option<Agent>> {
        let path = self.agent_dir(user_id, agent_id).join("agent.json");
        read_json(&path)
    }

    async fn save_agent(&self, agent: &Agent) -> Result<()> {
        let path = self
            .agent_dir(&agent.user_id, &agent.agent_id)
            .join("agent.json");
        write_json(&path, agent)
    }

    async fn delete_agent(&self, user_id: &str, agent_id: &str) -> Result<()> {
        let dir = self.agent_dir(user_id, agent_id);
        match std::fs::remove_dir_all(&dir) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(ErrorCode::internal(format!(
                "delete agent dir {}: {e}",
                dir.display()
            ))),
        }
    }

    async fn list_agents(&self, user_id: &str) -> Result<Vec<Agent>> {
        let agents_dir = self.root.join("users").join(user_id).join("agents");
        let entries = match std::fs::read_dir(&agents_dir) {
            Ok(rd) => rd,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => {
                return Err(ErrorCode::internal(format!(
                    "readdir {}: {e}",
                    agents_dir.display()
                )));
            }
        };
        let mut agents = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| ErrorCode::internal(format!("readdir entry: {e}")))?;
            let path = entry.path().join("agent.json");
            if let Some(agent) = read_json::<Agent>(&path)? {
                agents.push(agent);
            }
        }
        Ok(agents)
    }
}

// ── SkillRepo ─────────────────────────────────────────────────────────────────

#[async_trait]
impl SkillRepo for LocalFsBackend {
    async fn get_skill(
        &self,
        user_id: &str,
        agent_id: &str,
        skill_id: &str,
    ) -> Result<Option<Skill>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("skills")
            .join(format!("{skill_id}.json"));
        read_json(&path)
    }

    async fn save_skill(&self, skill: &Skill) -> Result<()> {
        let path = self
            .agent_dir(&skill.user_id, &skill.agent_id)
            .join("skills")
            .join(format!("{}.json", skill.skill_id));
        write_json(&path, skill)
    }

    async fn delete_skill(&self, user_id: &str, agent_id: &str, skill_id: &str) -> Result<()> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("skills")
            .join(format!("{skill_id}.json"));
        delete_file(&path)
    }

    async fn list_skills(&self, user_id: &str, agent_id: &str) -> Result<Vec<Skill>> {
        let dir = self.agent_dir(user_id, agent_id).join("skills");
        list_json_dir(&dir)
    }
}

// ── ChannelRepo ───────────────────────────────────────────────────────────────

#[async_trait]
impl ChannelRepo for LocalFsBackend {
    async fn get_channel(
        &self,
        user_id: &str,
        agent_id: &str,
        channel_id: &str,
    ) -> Result<Option<Channel>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("channels")
            .join(format!("{channel_id}.json"));
        read_json(&path)
    }

    async fn save_channel(&self, channel: &Channel) -> Result<()> {
        let path = self
            .agent_dir(&channel.user_id, &channel.agent_id)
            .join("channels")
            .join(format!("{}.json", channel.channel_id));
        write_json(&path, channel)
    }

    async fn delete_channel(&self, user_id: &str, agent_id: &str, channel_id: &str) -> Result<()> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("channels")
            .join(format!("{channel_id}.json"));
        delete_file(&path)
    }

    async fn list_channels(&self, user_id: &str, agent_id: &str) -> Result<Vec<Channel>> {
        let dir = self.agent_dir(user_id, agent_id).join("channels");
        list_json_dir(&dir)
    }
}

// ── SessionRepo ───────────────────────────────────────────────────────────────

#[async_trait]
impl SessionRepo for LocalFsBackend {
    async fn find_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Option<Session>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("sessions")
            .join(format!("{session_id}.json"));
        read_json(&path)
    }

    async fn find_latest_session(&self, user_id: &str, agent_id: &str) -> Result<Option<Session>> {
        let sessions = self.list_sessions(user_id, agent_id).await?;
        Ok(sessions
            .into_iter()
            .max_by(|a, b| a.updated_at.cmp(&b.updated_at)))
    }

    async fn create_session(&self, session: &Session) -> Result<()> {
        let path = self
            .agent_dir(&session.user_id, &session.agent_id)
            .join("sessions")
            .join(format!("{}.json", session.session_id));
        write_json(&path, session)
    }

    async fn update_session(&self, session: &Session) -> Result<()> {
        self.create_session(session).await
    }

    async fn list_sessions(&self, user_id: &str, agent_id: &str) -> Result<Vec<Session>> {
        let dir = self.agent_dir(user_id, agent_id).join("sessions");
        list_json_dir(&dir)
    }
}

// ── RunRepo ───────────────────────────────────────────────────────────────────

#[async_trait]
impl RunRepo for LocalFsBackend {
    async fn get_run(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        run_id: &str,
    ) -> Result<Option<Run>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("runs")
            .join(format!("{run_id}.json"));
        read_json(&path)
    }

    async fn save_run(&self, run: &Run) -> Result<()> {
        let path = self
            .agent_dir(&run.user_id, &run.agent_id)
            .join("runs")
            .join(format!("{}.json", run.run_id));
        write_json(&path, run)
    }

    async fn list_runs_by_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Vec<Run>> {
        let dir = self.agent_dir(user_id, agent_id).join("runs");
        let all: Vec<Run> = list_json_dir(&dir)?;
        Ok(all
            .into_iter()
            .filter(|r| r.session_id == session_id)
            .collect())
    }

    async fn load_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        run_id: &str,
    ) -> Result<Option<serde_json::Value>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("runs")
            .join(format!("{run_id}.handoff.json"));
        read_json(&path)
    }

    async fn save_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        run_id: &str,
        handoff: &serde_json::Value,
    ) -> Result<()> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("runs")
            .join(format!("{run_id}.handoff.json"));
        write_json(&path, handoff)
    }

    async fn clear_handoff(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        run_id: &str,
    ) -> Result<()> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("runs")
            .join(format!("{run_id}.handoff.json"));
        delete_file(&path)
    }

    async fn list_incomplete_runs(&self, user_id: &str, agent_id: &str) -> Result<Vec<Run>> {
        let dir = self.agent_dir(user_id, agent_id).join("runs");
        let all: Vec<Run> = list_json_dir(&dir)?;
        Ok(all
            .into_iter()
            .filter(|r| {
                r.status == RunStatus::Running.as_str() || r.status == RunStatus::Pending.as_str()
            })
            .collect())
    }
}

// ── RunEventRepo ──────────────────────────────────────────────────────────────

#[async_trait]
impl RunEventRepo for LocalFsBackend {
    async fn append_event(&self, event: &RunEvent) -> Result<()> {
        let path = self
            .agent_dir(&event.user_id, &event.agent_id)
            .join("run-events")
            .join(format!("{}.jsonl", event.run_id));
        append_jsonl(&path, event)
    }

    async fn list_events_by_run(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        run_id: &str,
    ) -> Result<Vec<RunEvent>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("run-events")
            .join(format!("{run_id}.jsonl"));
        read_jsonl(&path)
    }
}

// ── TraceRepo ─────────────────────────────────────────────────────────────────

#[async_trait]
impl TraceRepo for LocalFsBackend {
    async fn get_trace(
        &self,
        user_id: &str,
        agent_id: &str,
        trace_id: &str,
    ) -> Result<Option<Trace>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("traces")
            .join(format!("{trace_id}.json"));
        read_json(&path)
    }

    async fn save_trace(&self, trace: &Trace) -> Result<()> {
        let path = self
            .agent_dir(&trace.user_id, &trace.agent_id)
            .join("traces")
            .join(format!("{}.json", trace.trace_id));
        write_json(&path, trace)
    }

    async fn list_traces_by_run(
        &self,
        user_id: &str,
        agent_id: &str,
        _session_id: &str,
        _run_id: &str,
    ) -> Result<Vec<Trace>> {
        let dir = self.agent_dir(user_id, agent_id).join("traces");
        let all: Vec<Trace> = list_json_dir(&dir)?;
        Ok(all.into_iter().filter(|t| t.run_id == _run_id).collect())
    }

    async fn list_traces_by_session(
        &self,
        user_id: &str,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Vec<Trace>> {
        let dir = self.agent_dir(user_id, agent_id).join("traces");
        let all: Vec<Trace> = list_json_dir(&dir)?;
        Ok(all
            .into_iter()
            .filter(|t| t.session_id == session_id)
            .collect())
    }
}

// ── SpanRepo ──────────────────────────────────────────────────────────────────

#[async_trait]
impl SpanRepo for LocalFsBackend {
    async fn append_span(&self, span: &Span) -> Result<()> {
        let path = self
            .agent_dir(&span.user_id, &span.agent_id)
            .join("spans")
            .join(format!("{}.jsonl", span.trace_id));
        append_jsonl(&path, span)
    }

    async fn list_spans_by_trace(
        &self,
        user_id: &str,
        agent_id: &str,
        trace_id: &str,
    ) -> Result<Vec<Span>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("spans")
            .join(format!("{trace_id}.jsonl"));
        read_jsonl(&path)
    }
}

// ── TaskRepo ──────────────────────────────────────────────────────────────────

#[async_trait]
impl TaskRepo for LocalFsBackend {
    async fn get_task(&self, user_id: &str, agent_id: &str, task_id: &str) -> Result<Option<Task>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("tasks")
            .join(format!("{task_id}.json"));
        read_json(&path)
    }

    async fn save_task(&self, task: &Task) -> Result<()> {
        let path = self
            .agent_dir(&task.user_id, &task.agent_id)
            .join("tasks")
            .join(format!("{}.json", task.task_id));
        write_json(&path, task)
    }

    async fn delete_task(&self, user_id: &str, agent_id: &str, task_id: &str) -> Result<()> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("tasks")
            .join(format!("{task_id}.json"));
        delete_file(&path)
    }

    async fn list_tasks(&self, user_id: &str, agent_id: &str) -> Result<Vec<Task>> {
        let dir = self.agent_dir(user_id, agent_id).join("tasks");
        list_json_dir(&dir)
    }

    async fn update_task(&self, task: &Task) -> Result<()> {
        self.save_task(task).await
    }
}

// ── TaskHistoryRepo ───────────────────────────────────────────────────────────

#[async_trait]
impl TaskHistoryRepo for LocalFsBackend {
    async fn append_history(&self, entry: &TaskHistory) -> Result<()> {
        let path = self
            .agent_dir(&entry.user_id, &entry.agent_id)
            .join("task-history")
            .join(format!("{}.jsonl", entry.task_id));
        append_jsonl(&path, entry)
    }

    async fn list_history_by_task(
        &self,
        user_id: &str,
        agent_id: &str,
        task_id: &str,
    ) -> Result<Vec<TaskHistory>> {
        let path = self
            .agent_dir(user_id, agent_id)
            .join("task-history")
            .join(format!("{task_id}.jsonl"));
        read_jsonl(&path)
    }
}
