use serde::Deserialize;
use serde::Serialize;

/// Unified result envelope for all output formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub sequence: u64,
    pub timestamp: String,
    pub session_id: String,
    pub run_id: String,
    pub event_name: String,
    pub payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
}
