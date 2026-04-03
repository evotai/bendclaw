use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use super::service;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::state::AppState;
use crate::server::v1::common::ListQuery;
use crate::server::v1::common::Paginated;
use crate::tasks::input::TaskCreateSpec;
use crate::tasks::input::TaskUpdateSpec;
use crate::tasks::view::TaskHistoryView;
use crate::tasks::view::TaskSummaryView;
use crate::tasks::view::TaskView;

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    #[serde(flatten)]
    pub spec: TaskCreateSpec,
    pub node_id: Option<String>,
}

pub type UpdateTaskRequest = TaskUpdateSpec;

pub async fn list_tasks(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<TaskSummaryView>>> {
    let (records, total) = service::list_tasks(&state, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(TaskSummaryView::from).collect(),
        &q,
        total,
    )))
}

pub async fn create_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateTaskRequest>,
) -> Result<Json<TaskView>> {
    let record = service::create_task(&state, &agent_id, req).await?;
    Ok(Json(TaskView::from(record)))
}

pub async fn update_task(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
    Json(req): Json<UpdateTaskRequest>,
) -> Result<Json<TaskView>> {
    let record = service::update_task(&state, &agent_id, &task_id, req).await?;
    Ok(Json(TaskView::from(record)))
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
) -> Result<Json<TaskView>> {
    let record = service::toggle_task(&state, &agent_id, &task_id).await?;
    Ok(Json(TaskView::from(record)))
}

pub async fn list_task_history(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<TaskHistoryView>>> {
    let (records, total) = service::list_task_history(&state, &agent_id, &task_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(TaskHistoryView::from).collect(),
        &q,
        total,
    )))
}
