use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::response::Response;
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
pub struct RunEventResponse {
    pub seq: u32,
    pub event: String,
    pub payload: serde_json::Value,
    pub created_at: String,
}

#[derive(Serialize)]
pub struct RunResponse {
    pub id: String,
    pub session_id: String,
    pub status: String,
    pub input: String,
    pub output: String,
    pub error: String,
    pub metrics: serde_json::Value,
    pub stop_reason: String,
    pub iterations: u32,
    pub parent_run_id: String,
    pub node_id: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub events: Option<Vec<RunEventResponse>>,
}

#[derive(Deserialize, Default)]
pub struct RunsQuery {
    #[serde(flatten)]
    pub list: ListQuery,
    pub session_id: Option<String>,
    pub status: Option<String>,
    pub include_events: Option<bool>,
}

#[derive(Deserialize)]
pub struct CreateRunRequest {
    pub input: String,
    pub session_id: Option<String>,
    #[serde(default = "default_true")]
    pub stream: bool,
}

#[derive(Deserialize)]
pub struct ContinueRunRequest {
    pub input: Option<String>,
    pub session_id: Option<String>,
    #[serde(default = "default_true")]
    pub stream: bool,
}

fn default_true() -> bool {
    true
}

pub async fn list_runs(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<RunsQuery>,
) -> Result<Json<Paginated<RunResponse>>> {
    let data = service::list_runs(&state, &agent_id, q).await?;
    Ok(Json(data))
}

pub async fn list_runs_by_session(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, session_id)): Path<(String, String)>,
    Query(mut q): Query<RunsQuery>,
) -> Result<Json<Paginated<RunResponse>>> {
    q.session_id = Some(session_id);
    let data = service::list_runs(&state, &agent_id, q).await?;
    Ok(Json(data))
}

pub async fn get_run(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, run_id)): Path<(String, String)>,
) -> Result<Json<RunResponse>> {
    let data = service::get_run(&state, &agent_id, &run_id).await?;
    Ok(Json(data))
}

pub async fn create_run(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    headers: axum::http::HeaderMap,
    Json(req): Json<CreateRunRequest>,
) -> Result<Response> {
    let session_id = req.session_id.unwrap_or_else(crate::types::new_session_id);
    // parent_run_id is only accepted via internal header (set by BendclawClient),
    // not exposed in the public request body, to prevent lineage spoofing.
    let parent_run_id = headers
        .get("x-parent-run-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    service::execute_run(
        state,
        ctx,
        agent_id,
        session_id,
        req.input,
        req.stream,
        parent_run_id.clone(),
        None,
        parent_run_id.is_some(),
    )
    .await
}

pub async fn continue_run(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, run_id)): Path<(String, String)>,
    Json(req): Json<ContinueRunRequest>,
) -> Result<Response> {
    let base = service::load_run_record(&state, &agent_id, &run_id).await?;

    if base.status != "PAUSED" {
        return Err(ServiceError::Conflict(format!(
            "run is not paused (status={})",
            base.status
        )));
    }

    let input = req
        .input
        .unwrap_or_else(|| base.input.clone())
        .trim()
        .to_string();
    if input.is_empty() {
        return Err(ServiceError::BadRequest(
            "continue input must not be empty".to_string(),
        ));
    }

    let session_id = req.session_id.unwrap_or(base.session_id.clone());
    service::execute_run(
        state,
        ctx,
        agent_id,
        session_id,
        input,
        req.stream,
        Some(run_id.clone()),
        Some(run_id),
        false,
    )
    .await
}

pub async fn cancel_run(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, run_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let payload = service::cancel_run(&state, &agent_id, &run_id).await?;
    Ok(Json(payload))
}

pub async fn list_run_events(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, run_id)): Path<(String, String)>,
) -> Result<Json<Vec<RunEventResponse>>> {
    Ok(Json(
        service::list_run_events_standalone(&state, &agent_id, &run_id).await?,
    ))
}
