use axum::extract::Path;
use axum::extract::State;
use axum::Json;

use crate::agent_store::AgentStore;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::workbench::replay;

pub async fn get_replay(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, session_id)): Path<(String, String)>,
) -> Result<Json<replay::SessionReplaySummary>> {
    let pool = state.runtime.databases().agent_pool(&agent_id)?;
    let store = AgentStore::new(pool, state.runtime.llm());

    // Verify session exists and user owns it.
    let session = store
        .session_load(&session_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("session '{session_id}' not found")))?;
    if session.agent_id != agent_id || session.user_id != ctx.user_id {
        return Err(ServiceError::Forbidden(
            "session belongs to a different agent/user".to_string(),
        ));
    }

    let facts = replay::load_replay_facts(&store, &session_id).await?;
    let summary = replay::project_replay(&session_id, facts);
    Ok(Json(summary))
}
