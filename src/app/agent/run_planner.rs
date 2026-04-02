use super::request::AgentRequest;
use crate::types::entities::Session;

/// Pure data plan for a single run — no wiring, no driver assembly.
#[derive(Debug, Clone)]
pub struct RunPlan {
    pub session_id: String,
    pub agent_id: String,
    pub user_id: String,
    pub prompt: String,
    pub system_overlay: Option<String>,
    pub model: Option<String>,
    pub max_turns: Option<u32>,
    pub max_duration_secs: Option<u64>,
    pub tool_filter: Option<String>,
}

/// Build a RunPlan from the request and bound session.
pub fn build_run_plan(request: &AgentRequest, session: &Session) -> RunPlan {
    RunPlan {
        session_id: session.session_id.clone(),
        agent_id: session.agent_id.clone(),
        user_id: session.user_id.clone(),
        prompt: request.prompt.clone(),
        system_overlay: request.system_overlay.clone(),
        model: request.model.clone(),
        max_turns: request.max_turns,
        max_duration_secs: request.max_duration_secs,
        tool_filter: request.tool_filter.clone(),
    }
}
