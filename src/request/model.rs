/// Standard input model for agent execution — used by both CLI and HTTP.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub prompt: String,
    pub user_id: String,
    pub agent_id: String,
    pub session_id: Option<String>,
    pub resume_session: bool,
    pub model: Option<String>,
    pub system_overlay: Option<String>,
    pub max_turns: Option<u32>,
    pub max_duration_secs: Option<u64>,
    pub output_format: OutputFormat,
    pub tool_filter: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    StreamJson,
    Sse,
}
