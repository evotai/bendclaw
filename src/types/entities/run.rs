use std::fmt;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Cancelled,
    Error,
}

impl RunStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Running => "RUNNING",
            Self::Paused => "PAUSED",
            Self::Completed => "COMPLETED",
            Self::Cancelled => "CANCELLED",
            Self::Error => "ERROR",
        }
    }
}

impl fmt::Display for RunStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    UserTurn,
    SessionCheckpoint,
}

impl RunKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserTurn => "user_turn",
            Self::SessionCheckpoint => "session_checkpoint",
        }
    }
}

impl fmt::Display for RunKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub parent_run_id: String,
    #[serde(default)]
    pub root_trace_id: String,
    pub kind: String,
    pub status: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub output: serde_json::Value,
    #[serde(default)]
    pub error: serde_json::Value,
    #[serde(default)]
    pub metrics: serde_json::Value,
    #[serde(default)]
    pub stop_reason: String,
    #[serde(default)]
    pub iterations: u32,
    pub created_at: String,
    pub updated_at: String,
}
