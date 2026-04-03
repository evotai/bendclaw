use axum::extract::Path;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::context::RequestContext;
use super::error::ServiceError;
use super::state::AppState;

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
    s.runtime.setup_agent(&agent_id).await?;
    let database = s.runtime.agent_database_name(&agent_id)?;
    Ok(Json(SetupResponse { ok: true, database }))
}
