use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::ListQuery;
use crate::service::v1::common::Paginated;
use crate::storage::dal::task::TaskRecord;

#[derive(Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub agentos_id: String,
    pub name: String,
    pub cron_expr: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub last_run_at: String,
    pub next_run_at: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: TaskRecord) -> TaskResponse {
    TaskResponse {
        id: r.id,
        agentos_id: r.agentos_id,
        name: r.name,
        cron_expr: r.cron_expr,
        prompt: r.prompt,
        enabled: r.enabled,
        status: r.status,
        last_run_at: r.last_run_at,
        next_run_at: r.next_run_at,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub agentos_id: String,
    pub name: String,
    pub cron_expr: String,
    pub prompt: String,
}

#[derive(Deserialize)]
pub struct UpdateTaskRequest {
    pub name: Option<String>,
    pub cron_expr: Option<String>,
    pub prompt: Option<String>,
    pub enabled: Option<bool>,
}

pub async fn list_tasks(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<TaskResponse>>> {
    let (records, total) = service::list_tasks(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn create_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<Json<TaskResponse>> {
    let record = service::create_task(&state, &agent_id, req).await?;
    Ok(Json(to_response(record)))
}

pub async fn update_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<serde_json::Value>> {
    service::update_task(&state, &agent_id, &task_id, req).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

pub async fn delete_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    service::delete_task(&state, &agent_id, &task_id).await?;
    Ok(Json(serde_json::json!({ "deleted": task_id })))
}

pub async fn toggle_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
) -> Result<Json<TaskResponse>> {
    let record = service::toggle_task(&state, &agent_id, &task_id).await?;
    Ok(Json(to_response(record)))
}
