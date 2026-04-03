use std::sync::Arc;

use super::run_planner::RunPlan;
use crate::app::result::event_envelope::EventEnvelope;
use crate::storage::run_events::RunEventRepo;
use crate::storage::runs::RunRepo;
use crate::types::entities::Run;
use crate::types::entities::RunEvent;
use crate::types::entities::RunEventKind;
use crate::types::entities::RunStatus;
use crate::types::id::new_id;
use crate::types::id::new_run_id;
use crate::types::Result;

/// Execute a run and return a vec of EventEnvelopes.
///
/// This is the single run owner at the app layer. It creates the Run entity,
/// records RunEvents, and finalizes the run. In future phases, this will call
/// `kernel::run::run_entry::start_run()` and stream events. For now, it
/// records the user input as a RunEvent and returns the envelope sequence.
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
