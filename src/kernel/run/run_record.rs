//! Run record initialization for a chat turn.

use crate::base::Result;
use crate::kernel::agent_store::AgentStore;
use crate::storage::dal::run::record::RunRecord;
use crate::storage::dal::run::record::RunStatus;

/// Create session record (first turn) and run record. Returns the run_id.
pub(crate) async fn init_run(
    storage: &AgentStore,
    session_id: &str,
    agent_id: &str,
    user_id: &str,
    user_message: &str,
    parent_run_id: Option<&str>,
) -> Result<String> {
    if storage.session_load(session_id).await?.is_none() {
        if let Err(e) = storage
            .session_upsert(session_id, agent_id, user_id, Some(user_message), None)
            .await
        {
            tracing::warn!(log_kind = "server_log", stage = "persist", action = "init_run", status = "session_upsert_failed", session_id, agent_id, user_id, error = %e, "failed to insert session record");
        }
    }

    let run_id = crate::kernel::new_run_id();
    if let Err(e) = storage
        .run_insert(&RunRecord {
            id: run_id.clone(),
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            parent_run_id: parent_run_id.unwrap_or_default().to_string(),
            status: RunStatus::Running.as_str().to_string(),
            input: user_message.to_string(),
            output: String::new(),
            error: String::new(),
            metrics: String::new(),
            stop_reason: String::new(),
            iterations: 0,
            created_at: String::new(),
            updated_at: String::new(),
        })
        .await
    {
        tracing::warn!(log_kind = "server_log", stage = "persist", action = "init_run", status = "run_insert_failed", session_id, agent_id, user_id, run_id = %run_id, parent_run_id = parent_run_id.unwrap_or_default(), error = %e, "failed to insert run record");
    }

    Ok(run_id)
}
