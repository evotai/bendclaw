//! CLI-specific orchestration: plan + execute a run from AgentRequest.
//!
//! This is the CLI-only pipeline glue. Server mode uses `binding::run_binding`
//! which goes through `Runtime::invoke()`.

use std::sync::Arc;

use crate::app::result::event_envelope::EventEnvelope;
use crate::request::AgentRequest;
use crate::storage::run_events::RunEventRepo;
use crate::storage::runs::RunRepo;
use crate::types::entities::Run;
use crate::types::entities::RunEvent;
use crate::types::entities::RunEventKind;
use crate::types::entities::RunStatus;
use crate::types::entities::Session;
use crate::types::id::new_id;
use crate::types::id::new_run_id;
use crate::types::Result;

// ── Plan ──────────────────────────────────────────────────────────────────────

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

// ── Execute ───────────────────────────────────────────────────────────────────

/// Execute a run and return a vec of EventEnvelopes.
///
/// Creates the Run entity, records RunEvents, and finalizes the run.
pub async fn execute_run(
    run_repo: &Arc<dyn RunRepo>,
    run_event_repo: &Arc<dyn RunEventRepo>,
    plan: &RunPlan,
) -> Result<Vec<EventEnvelope>> {
    let run_id = new_run_id();
    let now = chrono::Utc::now().to_rfc3339();

    let run = Run {
        run_id: run_id.clone(),
        session_id: plan.session_id.clone(),
        agent_id: plan.agent_id.clone(),
        user_id: plan.user_id.clone(),
        parent_run_id: String::new(),
        root_trace_id: String::new(),
        kind: "user_turn".into(),
        status: RunStatus::Running.as_str().into(),
        input: serde_json::json!({"prompt": &plan.prompt}),
        output: serde_json::Value::Null,
        error: serde_json::Value::Null,
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    run_repo.save_run(&run).await?;

    let input_event = RunEvent {
        event_id: new_id(),
        run_id: run_id.clone(),
        session_id: plan.session_id.clone(),
        agent_id: plan.agent_id.clone(),
        user_id: plan.user_id.clone(),
        seq: 1,
        kind: RunEventKind::UserInput,
        payload: serde_json::json!({"prompt": &plan.prompt}),
        created_at: now.clone(),
    };
    run_event_repo.append_event(&input_event).await?;

    let envelopes = vec![EventEnvelope {
        sequence: 1,
        timestamp: now,
        session_id: plan.session_id.clone(),
        run_id: run_id.clone(),
        event_name: "user.input".into(),
        payload: serde_json::json!({"prompt": &plan.prompt}),
        cursor: None,
    }];

    Ok(envelopes)
}
