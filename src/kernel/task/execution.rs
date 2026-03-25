use crate::base::new_id;
use crate::base::Result;
use crate::observability::log::slog;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::dal::task_history::TaskHistoryRecord;
use crate::storage::dal::task_history::TaskHistoryRepo;
use crate::storage::pool::Pool;

/// Record execution results and update task state.
#[allow(clippy::too_many_arguments)]
pub async fn finish_execution(
    pool: &Pool,
    task: &TaskRecord,
    lease_token: &str,
    node_id: &str,
    status: &str,
    run_id: Option<String>,
    output: Option<String>,
    error: Option<String>,
    duration_ms: i32,
    delivery_status: Option<String>,
    delivery_error: Option<String>,
) -> Result<()> {
    // 1. Write history record
    let history = TaskHistoryRecord {
        id: new_id(),
        task_id: task.id.clone(),
        run_id,
        task_name: task.name.clone(),
        schedule: task.schedule.clone(),
        prompt: task.prompt.clone(),
        status: status.to_string(),
        output,
        error: error.clone(),
        duration_ms: Some(duration_ms),
        delivery: task.delivery.clone(),
        delivery_status,
        delivery_error,
        user_id: task.user_id.clone(),
        executed_by_node_id: Some(node_id.to_string()),
        created_at: String::new(),
    };
    let history_repo = TaskHistoryRepo::new(pool.clone());
    if let Err(e) = history_repo.insert(&history).await {
        slog!(error, "task", "history_failed", task_id = task.id, error = %e,);
    }

    // 2. Compute next_run_at from schedule
    let next_run_at = task.schedule.next_run_at();

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
    if task.delete_after_run && matches!(task.schedule, TaskSchedule::At { .. }) {
        task_repo.delete(&task.id).await?;
    }

    Ok(())
}
