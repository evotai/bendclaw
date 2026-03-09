use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceRecord {
    pub trace_id: String,
    pub run_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub name: String,
    pub status: String,
    pub duration_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_cost: f64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanRecord {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: String,
    pub name: String,
    pub kind: String,
    pub model_role: String,
    pub status: String,
    pub duration_ms: u64,
    pub ttft_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub cost: f64,
    pub error_code: String,
    pub error_message: String,
    pub summary: String,
    pub meta: String,
    pub created_at: String,
}
