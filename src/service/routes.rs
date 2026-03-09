use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::context::RequestContext;
use super::error::ServiceError;
use super::state::AppState;
use crate::kernel::session::SessionStats;

type Result<T> = std::result::Result<T, ServiceError>;

#[derive(Serialize)]
pub struct SetupResponse {
    pub ok: bool,
    pub database: String,
}

pub async fn setup_agent(
    State(s): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<SetupResponse>> {
    s.runtime
        .setup_agent(&agent_id)
        .await
        .map_err(ServiceError::from)?;
    let database = s.runtime.agent_database_name(&agent_id);
    Ok(Json(SetupResponse { ok: true, database }))
}

pub async fn session_stats(State(s): State<AppState>) -> Result<Json<SessionStats>> {
    Ok(Json(s.runtime.sessions().stats()))
}

#[derive(Serialize)]
pub struct CanSuspendResponse {
    pub can_suspend: bool,
}

pub async fn can_suspend(State(s): State<AppState>) -> Result<Json<CanSuspendResponse>> {
    Ok(Json(CanSuspendResponse {
        can_suspend: s.runtime.sessions().can_suspend(),
    }))
}
