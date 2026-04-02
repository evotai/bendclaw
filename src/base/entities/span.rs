use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub span_id: String,
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    #[serde(default)]
    pub parent_span_id: String,
    pub name: String,
    pub kind: String,
    pub status: String,
    pub created_at: String,
    /// All statistics, metrics, and diagnostic detail — single VARIANT column.
    /// Contains: model_role, duration_ms, ttft_ms, input_tokens, output_tokens,
    /// reasoning_tokens, cost, error_code, error_message, summary, meta.
    #[serde(default)]
    pub doc: serde_json::Value,
}
