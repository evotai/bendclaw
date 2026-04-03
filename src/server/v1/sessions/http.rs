use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;
use crate::server::v1::common::ListQuery;
use crate::server::v1::common::Paginated;

#[derive(Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub title: String,
    pub base_key: String,
    pub replaced_by_session_id: String,
    pub reset_reason: String,
    pub session_state: serde_json::Value,
    pub meta: serde_json::Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize, Default)]
pub struct SessionsQuery {
    #[serde(flatten)]
    pub list: ListQuery,
    pub search: Option<String>,
}

#[derive(Deserialize)]
pub struct CreateSessionRequest {
    pub title: Option<String>,
    pub session_state: Option<serde_json::Value>,
}

#[derive(Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub session_state: Option<serde_json::Value>,
}

pub async fn list_sessions(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<SessionsQuery>,
) -> Result<Json<Paginated<SessionResponse>>> {
    let response = service::list_sessions(&state, &ctx.user_id, &agent_id, q).await?;
    Ok(Json(response))
}

pub async fn get_session(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, session_id)): Path<(String, String)>,
) -> Result<Json<SessionResponse>> {
    let response = service::get_session(&state, &agent_id, &session_id).await?;
    Ok(Json(response))
}

pub async fn create_session(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<SessionResponse>> {
    let response = service::create_session(
        &state,
        &agent_id,
        &ctx.user_id,
        req.title.as_deref(),
        req.session_state.as_ref(),
    )
    .await?;
    Ok(Json(response))
}

pub async fn update_session(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, session_id)): Path<(String, String)>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<SessionResponse>> {
    let existing = service::load_session_record(&state, &agent_id, &session_id).await?;
    let existing = existing
        .ok_or_else(|| ServiceError::AgentNotFound(format!("session '{session_id}' not found")))?;
    let response =
        service::update_session(&state, &session_id, &existing, req.title, req.session_state)
            .await?;
    Ok(Json(response))
}

pub async fn delete_session(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, session_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    service::delete_session(&state, &agent_id, &session_id).await?;
    Ok(Json(serde_json::json!({ "deleted": session_id })))
}
