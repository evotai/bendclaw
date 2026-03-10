use crate::base::new_id;
use crate::base::Result;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::dal::task_history::TaskHistoryRecord;
use crate::storage::dal::task_history::TaskHistoryRepo;
use crate::storage::pool::Pool;

/// Atomically claim all due tasks for this executor instance.
/// Returns the claimed tasks and the lease token used.
pub async fn claim_due_tasks(
    pool: &Pool,
    executor_instance_id: &str,
) -> Result<(Vec<TaskRecord>, String)> {
    let lease_token = new_id();
    let repo = TaskRepo::new(pool.clone());
    let tasks = repo
        .claim_due_tasks(executor_instance_id, &lease_token)
        .await?;
    Ok((tasks, lease_token))
}

/// Record execution results and update task state.
#[allow(clippy::too_many_arguments)]
pub async fn finish_execution(
    pool: &Pool,
    task: &TaskRecord,
    lease_token: &str,
    executor_instance_id: &str,
    status: &str,
    run_id: Option<String>,
    output: Option<String>,
    error: Option<String>,
    duration_ms: i32,
    webhook_status: Option<String>,
    webhook_error: Option<String>,
) -> Result<()> {
    // 1. Write history record
    let history = TaskHistoryRecord {
        id: new_id(),
        task_id: task.id.clone(),
        run_id,
        task_name: task.name.clone(),
        schedule_kind: task.schedule_kind.clone(),
        cron_expr: if task.cron_expr.is_empty() {
            None
        } else {
            Some(task.cron_expr.clone())
        },
        prompt: task.prompt.clone(),
        status: status.to_string(),
        output,
        error: error.clone(),
        duration_ms: Some(duration_ms),
        webhook_url: task.webhook_url.clone(),
        webhook_status,
        webhook_error,
        executed_by_instance_id: Some(executor_instance_id.to_string()),
        created_at: String::new(),
    };
    let history_repo = TaskHistoryRepo::new(pool.clone());
    if let Err(e) = history_repo.insert(&history).await {
        tracing::error!(task_id = task.id, error = %e, "failed to write task history");
    }

    // 2. Compute next_run_at from schedule
    let schedule = TaskSchedule::from_record(
        &task.schedule_kind,
        &task.cron_expr,
        task.every_seconds,
        task.at_time.as_deref(),
        task.tz.as_deref(),
    );
    let next_run_at = schedule.and_then(|s| s.next_run_at());

    // 3. Finish task with lease verification
    let task_repo = TaskRepo::new(pool.clone());
    task_repo
        .finish_task(
            &task.id,
            lease_token,
            status,
            error.as_deref(),
            next_run_at.as_deref(),
        )
        .await?;

    // 4. Auto-delete one-shot tasks
    if task.delete_after_run && task.schedule_kind == "at" {
        tracing::info!(task_id = task.id, "deleting one-shot task after run");
        task_repo.delete(&task.id).await?;
    }

    Ok(())
}
