use crate::base::new_id;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::dal::task_history::TaskHistoryRecord;
use crate::storage::dal::task_history::TaskHistoryRepo;
use crate::storage::pool::Pool;

pub struct CreateTaskParams {
    pub executor_instance_id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: TaskSchedule,
    pub webhook_url: Option<String>,
    pub delete_after_run: bool,
}

pub struct UpdateTaskParams {
    pub name: Option<String>,
    pub prompt: Option<String>,
    pub schedule: Option<TaskSchedule>,
    pub enabled: Option<bool>,
    pub webhook_url: Option<Option<String>>,
    pub delete_after_run: Option<bool>,
}

pub async fn create_task(pool: &Pool, params: CreateTaskParams) -> Result<TaskRecord> {
    params
        .schedule
        .validate()
        .map_err(|e| ErrorCode::invalid_input(e))?;

    let next_run_at = params.schedule.initial_next_run_at();
    let mut record = TaskRecord {
        id: new_id(),
        executor_instance_id: params.executor_instance_id,
        name: params.name,
        cron_expr: String::new(),
        prompt: params.prompt,
        enabled: true,
        status: "idle".to_string(),
        schedule_kind: String::new(),
        every_seconds: None,
        at_time: None,
        tz: None,
        webhook_url: params.webhook_url,
        last_error: None,
        delete_after_run: params.delete_after_run,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at,
        lease_token: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    params.schedule.apply_to_record(&mut record);

    let repo = TaskRepo::new(pool.clone());
    repo.insert(&record).await?;
    Ok(record)
}

pub async fn list_tasks(pool: &Pool, limit: u32) -> Result<Vec<TaskRecord>> {
    let repo = TaskRepo::new(pool.clone());
    repo.list(limit).await
}

pub async fn get_task(pool: &Pool, task_id: &str) -> Result<Option<TaskRecord>> {
    let repo = TaskRepo::new(pool.clone());
    repo.get(task_id).await
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

    // Determine effective schedule and whether it changed
    let (schedule_changed, next_run_at) = if let Some(ref schedule) = params.schedule {
        schedule
            .validate()
            .map_err(|e| ErrorCode::invalid_input(e))?;
        (true, schedule.initial_next_run_at())
    } else {
        (false, None)
    };

    // Build repo update args from params
    let schedule_kind;
    let cron_expr;
    let every_seconds;
    let at_time_val;
    let tz_val;
    if let Some(ref schedule) = params.schedule {
        schedule_kind = Some(schedule.kind_str().to_string());
        match schedule {
            TaskSchedule::Cron { expr, tz } => {
                cron_expr = Some(expr.clone());
                every_seconds = Some(None);
                at_time_val = Some(None);
                tz_val = Some(tz.as_deref().map(|s| s.to_string()));
            }
            TaskSchedule::Every { seconds } => {
                cron_expr = Some(String::new());
                every_seconds = Some(Some(*seconds));
                at_time_val = Some(None);
                tz_val = Some(None);
            }
            TaskSchedule::At { time } => {
                cron_expr = Some(String::new());
                every_seconds = Some(None);
                at_time_val = Some(Some(time.clone()));
                tz_val = Some(None);
            }
        }
    } else {
        schedule_kind = None;
        cron_expr = None;
        every_seconds = None;
        at_time_val = None;
        tz_val = None;
    }

    let next_run_at_outer: Option<Option<String>> = if schedule_changed {
        Some(next_run_at)
    } else {
        None
    };

    repo.update(
        task_id,
        params.name.as_deref(),
        cron_expr.as_deref(),
        params.prompt.as_deref(),
        params.enabled,
        schedule_kind.as_deref(),
        every_seconds,
        at_time_val.as_ref().map(|v| v.as_deref()),
        tz_val.as_ref().map(|v| v.as_deref()),
        params.webhook_url.as_ref().map(|v| v.as_deref()),
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

pub async fn list_task_history(
    pool: &Pool,
    task_id: &str,
    limit: u32,
) -> Result<Vec<TaskHistoryRecord>> {
    let repo = TaskHistoryRepo::new(pool.clone());
    repo.list_by_task(task_id, limit).await
}
