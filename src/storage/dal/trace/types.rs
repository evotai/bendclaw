use serde::Serialize;

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentTraceSummary {
    pub agent_id: String,
    pub trace_count: i64,
    pub llm_calls: i64,
    pub tool_calls: i64,
    pub skill_calls: i64,
    pub error_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost: f64,
    pub avg_duration_ms: f64,
    pub last_active: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentTraceBreakdown {
    pub name: String,
    pub calls: i64,
    pub errors: i64,
    pub avg_duration_ms: f64,
    pub total_cost: f64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct AgentTraceDetails {
    pub agent_id: String,
    pub trace_count: i64,
    pub llm_calls: i64,
    pub tool_calls: i64,
    pub skill_calls: i64,
    pub error_count: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost: f64,
    pub avg_duration_ms: f64,
    pub last_active: String,
    pub llm: Vec<AgentTraceBreakdown>,
    pub tools: Vec<AgentTraceBreakdown>,
    pub skills: Vec<AgentTraceBreakdown>,
    pub errors: Vec<AgentTraceBreakdown>,
    pub recent_trace_ids: Vec<String>,
}
