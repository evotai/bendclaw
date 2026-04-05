use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub session_id: String,
    pub cwd: String,
    pub model: String,
    pub title: Option<String>,
    pub turns: u32,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionMeta {
    pub fn new(session_id: String, cwd: String, model: String) -> Self {
        let now = Utc::now().to_rfc3339();
        Self {
            session_id,
            cwd,
            model,
            title: None,
            turns: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptKind {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub session_id: String,
    pub run_id: Option<String>,
    pub seq: u64,
    pub turn: u32,
    pub kind: TranscriptKind,
    pub message: bend_agent::Message,
    pub created_at: String,
}

impl TranscriptEntry {
    pub fn new(
        session_id: String,
        run_id: Option<String>,
        seq: u64,
        turn: u32,
        message: bend_agent::Message,
    ) -> Self {
        let kind = match message.role {
            bend_agent::MessageRole::User => TranscriptKind::User,
            bend_agent::MessageRole::Assistant => TranscriptKind::Assistant,
        };

        Self {
            session_id,
            run_id,
            seq,
            turn,
            kind,
            message,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListSessions {
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTranscriptEntries {
    pub session_id: String,
    pub run_id: Option<String>,
    pub after_seq: Option<u64>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunMeta {
    pub run_id: String,
    pub session_id: String,
    pub status: RunStatus,
    pub model: String,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl RunMeta {
    pub fn new(run_id: String, session_id: String, model: String) -> Self {
        Self {
            run_id,
            session_id,
            status: RunStatus::Running,
            model,
            started_at: Utc::now().to_rfc3339(),
            finished_at: None,
        }
    }

    pub fn finish(&mut self, status: RunStatus) {
        self.status = status;
        self.finished_at = Some(Utc::now().to_rfc3339());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    RunStarted,
    System,
    AssistantMessage,
    ToolResult,
    PartialMessage,
    CompactBoundary,
    Status,
    TaskNotification,
    RateLimit,
    Progress,
    Error,
    RunFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub event_id: String,
    pub run_id: String,
    pub session_id: String,
    pub turn: u32,
    pub kind: RunEventKind,
    pub payload: serde_json::Value,
    pub created_at: String,
}

impl RunEvent {
    pub fn new(
        run_id: String,
        session_id: String,
        turn: u32,
        kind: RunEventKind,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            event_id: crate::ids::new_id(),
            run_id,
            session_id,
            turn,
            kind,
            payload,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListRuns {
    pub session_id: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListRunEvents {
    pub run_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceMeta {
    pub trace_id: String,
    pub session_id: String,
    pub run_id: String,
    pub status: TraceStatus,
    pub started_at: String,
    pub finished_at: Option<String>,
}

impl TraceMeta {
    pub fn new(trace_id: String, session_id: String, run_id: String) -> Self {
        Self {
            trace_id,
            session_id,
            run_id,
            status: TraceStatus::Running,
            started_at: Utc::now().to_rfc3339(),
            finished_at: None,
        }
    }

    pub fn finish(&mut self, status: TraceStatus) {
        self.status = status;
        self.finished_at = Some(Utc::now().to_rfc3339());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceEventKind {
    SessionStarted,
    SessionFinished,
    LlmRequest,
    LlmResponse,
    ToolStarted,
    ToolFinished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceEvent {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub kind: TraceEventKind,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ListTraces {
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListTraceEvents {
    pub trace_id: String,
}
