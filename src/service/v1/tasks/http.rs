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
use crate::storage::dal::task::TaskSchedule;
use crate::storage::dal::task_history::TaskHistoryRecord;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ScheduleRequest {
    Cron { expr: String, tz: Option<String> },
    Every { seconds: i32 },
    At { time: String },
}

impl ScheduleRequest {
    pub fn into_task_schedule(self) -> TaskSchedule {
        match self {
            ScheduleRequest::Cron { expr, tz } => TaskSchedule::Cron { expr, tz },
            ScheduleRequest::Every { seconds } => TaskSchedule::Every { seconds },
            ScheduleRequest::At { time } => TaskSchedule::At { time },
        }
    }
}

#[derive(Deserialize)]
pub struct CreateTaskRequest {
    pub name: String,
    pub prompt: String,
    pub schedule: ScheduleRequest,
    pub executor_instance_id: Option<String>,
    pub webhook_url: Option<String>,
    #[serde(default)]
    pub delete_after_run: bool,
}

#[derive(Deserialize)]
pub struct UpdateTaskRequest {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<ScheduleRequest>,
    pub enabled: Option<bool>,
    pub webhook_url: Option<Option<String>>,
    pub delete_after_run: Option<bool>,
}

#[derive(Serialize)]
pub struct TaskResponse {
    pub id: String,
    pub executor_instance_id: String,
    pub name: String,
    pub cron_expr: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub schedule_kind: String,
    pub every_seconds: Option<i32>,
    pub at_time: Option<String>,
    pub tz: Option<String>,
    pub webhook_url: Option<String>,
    pub last_error: Option<String>,
    pub delete_after_run: bool,
    pub run_count: i32,
    pub last_run_at: String,
    pub next_run_at: Option<String>,
    pub lease_token: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(r: TaskRecord) -> TaskResponse {
    TaskResponse {
        id: r.id,
        executor_instance_id: r.executor_instance_id,
        name: r.name,
        cron_expr: r.cron_expr,
        prompt: r.prompt,
        enabled: r.enabled,
        status: r.status,
        schedule_kind: r.schedule_kind,
        every_seconds: r.every_seconds,
        at_time: r.at_time,
        tz: r.tz,
        webhook_url: r.webhook_url,
        last_error: r.last_error,
        delete_after_run: r.delete_after_run,
        run_count: r.run_count,
        last_run_at: r.last_run_at,
        next_run_at: r.next_run_at,
        lease_token: r.lease_token,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

#[derive(Serialize)]
pub struct TaskHistoryResponse {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub task_name: String,
    pub schedule_kind: String,
    pub cron_expr: Option<String>,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i32>,
    pub webhook_url: Option<String>,
    pub webhook_status: Option<String>,
    pub webhook_error: Option<String>,
    pub executed_by_instance_id: Option<String>,
    pub created_at: String,
}

fn to_history_response(r: TaskHistoryRecord) -> TaskHistoryResponse {
    TaskHistoryResponse {
        id: r.id,
        task_id: r.task_id,
        run_id: r.run_id,
        task_name: r.task_name,
        schedule_kind: r.schedule_kind,
        cron_expr: r.cron_expr,
        prompt: r.prompt,
        status: r.status,
        output: r.output,
        error: r.error,
        duration_ms: r.duration_ms,
        webhook_url: r.webhook_url,
        webhook_status: r.webhook_status,
        webhook_error: r.webhook_error,
        executed_by_instance_id: r.executed_by_instance_id,
        created_at: r.created_at,
    }
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
) -> Result<Json<TaskResponse>> {
    let record = service::update_task(&state, &agent_id, &task_id, req).await?;
    Ok(Json(to_response(record)))
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

pub async fn list_task_history(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path((agent_id, task_id)): Path<(String, String)>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<TaskHistoryResponse>>> {
    let (records, total) = service::list_task_history(&state, &agent_id, &task_id, &q).await?;
    Ok(Json(Paginated::new(
        records.into_iter().map(to_history_response).collect(),
        &q,
        total,
    )))
}
