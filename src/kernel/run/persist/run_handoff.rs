use serde::Deserialize;
use serde::Serialize;

/// Resumable execution summary — written incrementally during a run,
/// consumed by `run_entry` on resume.
///
/// This is NOT a transcript copy. It contains only the minimum state needed
/// to resume or summarize an interrupted run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunHandoff {
    pub run_id: String,
    pub session_id: String,
    pub last_turn: u32,
    #[serde(default)]
    pub pending_tool_calls: Vec<String>,
    #[serde(default)]
    pub compaction_checkpoint: Option<serde_json::Value>,
    #[serde(default)]
    pub partial_output: String,
}

impl RunHandoff {
    pub fn to_json(&self) -> crate::types::Result<serde_json::Value> {
        serde_json::to_value(self)
            .map_err(|e| crate::types::ErrorCode::internal(format!("serialize handoff: {e}")))
    }

    pub fn from_json(val: &serde_json::Value) -> crate::types::Result<Self> {
        serde_json::from_value(val.clone())
            .map_err(|e| crate::types::ErrorCode::internal(format!("parse handoff: {e}")))
    }
}
