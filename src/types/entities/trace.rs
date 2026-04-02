use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub parent_trace_id: String,
    pub name: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    /// Statistics and metrics — kept in a single doc for VARIANT mapping.
    /// Contains: duration_ms, input_tokens, output_tokens, total_cost, origin_node_id, etc.
    #[serde(default)]
    pub doc: serde_json::Value,
}
