use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::pool::Pool;
use crate::types::new_id;
use crate::types::ErrorCode;
use crate::types::Result;

pub struct CreateTaskParams {
    pub node_id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: TaskSchedule,
    pub delivery: TaskDelivery,
    pub delete_after_run: bool,
    pub user_id: String,
    pub scope: String,
    pub created_by: String,
}

pub struct UpdateTaskParams {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<TaskSchedule>,
    pub enabled: Option<bool>,
    pub delivery: Option<TaskDelivery>,
    pub delete_after_run: Option<bool>,
}

pub async fn create_task(pool: &Pool, params: CreateTaskParams) -> Result<TaskRecord> {
    params
        .schedule
        .validate()
        .map_err(|e| ErrorCode::invalid_input(e))?;
    params
        .delivery
        .validate()
        .map_err(|e| ErrorCode::invalid_input(e))?;

    let next_run_at = params.schedule.initial_next_run_at();
    let record = TaskRecord {
        id: new_id(),
        node_id: params.node_id,
        name: params.name,
        prompt: params.prompt,
        enabled: true,
        status: "idle".to_string(),
        schedule: params.schedule,
        delivery: params.delivery,
        user_id: params.user_id,
        scope: params.scope,
        created_by: params.created_by,
        last_error: None,
        delete_after_run: params.delete_after_run,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at,
        lease_token: None,
        lease_node_id: None,
        lease_expires_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    };

    let repo = TaskRepo::new(pool.clone());
    repo.insert(&record).await?;
    Ok(record)
}

pub async fn update_task(
    pool: &Pool,
    task_id: &str,
    params: UpdateTaskParams,
) -> Result<TaskRecord> {
    let repo = TaskRepo::new(pool.clone());
    let _current = repo
        .get(task_id)
        .await?
        .ok_or_else(|| ErrorCode::not_found(format!("task {task_id} not found")))?;

    let (schedule_changed, next_run_at) = if let Some(ref schedule) = params.schedule {
        schedule
            .validate()
            .map_err(|e| ErrorCode::invalid_input(e))?;
        (true, schedule.initial_next_run_at())
    } else {
        (false, None)
    };
    if let Some(ref delivery) = params.delivery {
        delivery
            .validate()
            .map_err(|e| ErrorCode::invalid_input(e))?;
    }

    let next_run_at_outer: Option<Option<String>> = if schedule_changed {
        Some(next_run_at)
    } else {
        None
    };

    repo.update(
        task_id,
        params.name.as_deref(),
        params.prompt.as_deref(),
        params.enabled,
        params.schedule.as_ref(),
        params.delivery.as_ref(),
        params.delete_after_run,
        next_run_at_outer.as_ref().map(|v| v.as_deref()),
    )
    .await?;

    let updated = repo
        .get(task_id)
        .await?
        .ok_or_else(|| ErrorCode::not_found(format!("task {task_id} not found after update")))?;
    Ok(updated)
}

pub async fn delete_task(pool: &Pool, task_id: &str) -> Result<()> {
    let repo = TaskRepo::new(pool.clone());
    repo.delete(task_id).await
}

pub async fn toggle_task(pool: &Pool, task_id: &str) -> Result<TaskRecord> {
    let repo = TaskRepo::new(pool.clone());
    repo.toggle(task_id).await?;
    repo.get(task_id)
        .await?
        .ok_or_else(|| ErrorCode::not_found(format!("task {task_id} not found")))
}
