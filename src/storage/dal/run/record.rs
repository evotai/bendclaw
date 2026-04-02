use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    #[serde(rename = "PENDING")]
    Pending,
    #[serde(rename = "RUNNING")]
    Running,
    #[serde(rename = "PAUSED")]
    Paused,
    #[serde(rename = "COMPLETED")]
    Completed,
    #[serde(rename = "CANCELLED")]
    Cancelled,
    #[serde(rename = "ERROR")]
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

impl std::fmt::Display for RunStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunKind {
    #[serde(rename = "user_turn")]
    UserTurn,
    #[serde(rename = "session_checkpoint")]
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

impl std::fmt::Display for RunKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunMetrics {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub reasoning_tokens: u64,
    pub total_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub ttft_ms: u64,
    pub duration_ms: u64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub kind: String,
    pub parent_run_id: String,
    pub node_id: String,
    pub status: String,
    pub input: String,
    pub output: String,
    pub error: String,
    pub metrics: String,
    pub stop_reason: String,
    pub checkpoint_through_run_id: String,
    pub iterations: u32,
    pub created_at: String,
    pub updated_at: String,
}

impl RunRecord {
    pub fn is_session_checkpoint(&self) -> bool {
        self.kind == RunKind::SessionCheckpoint.as_str()
    }

    pub fn parse_metrics(&self) -> crate::types::Result<RunMetrics> {
        if self.metrics.is_empty() {
            return Ok(RunMetrics::default());
        }
        crate::storage::sql::parse_json(&self.metrics, "runs.metrics")
    }

    pub fn metrics_json(&self) -> crate::types::Result<serde_json::Value> {
        if self.metrics.is_empty() {
            return Ok(serde_json::Value::Null);
        }
        crate::storage::sql::parse_json(&self.metrics, "runs.metrics")
    }
}
