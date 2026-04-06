use chrono::Utc;
use serde::Deserialize;
use serde::Serialize;

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
