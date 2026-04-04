use std::sync::Arc;

use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

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
    pub payload: Value,
    pub created_at: String,
}

impl RunEvent {
    pub fn new(
        run_id: String,
        session_id: String,
        turn: u32,
        kind: RunEventKind,
        payload: Value,
    ) -> Self {
        Self {
            event_id: ulid::Ulid::new().to_string(),
            run_id,
            session_id,
            turn,
            kind,
            payload,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    pub fn payload_as<T: DeserializeOwned>(&self) -> Option<T> {
        crate::run::payload::payload_as(&self.payload)
    }
}

pub type RunEventArc = Arc<RunEvent>;
