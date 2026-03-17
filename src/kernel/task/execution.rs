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
    executor_node_id: &str,
) -> Result<(Vec<TaskRecord>, String)> {
    let lease_token = new_id();
    let repo = TaskRepo::new(pool.clone());
    let tasks = repo.claim_due_tasks(executor_node_id, &lease_token).await?;
    Ok((tasks, lease_token))
}

/// Record execution results and update task state.
#[allow(clippy::too_many_arguments)]
pub async fn finish_execution(
    pool: &Pool,
    task: &TaskRecord,
    lease_token: &str,
    executor_node_id: &str,
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
        executed_by_node_id: Some(executor_node_id.to_string()),
        created_at: String::new(),
    };
    let history_repo = TaskHistoryRepo::new(pool.clone());
    if let Err(e) = history_repo.insert(&history).await {
        tracing::error!(task_id = task.id, error = %e, "failed to write task history");
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
        tracing::info!(task_id = task.id, "deleting one-shot task after run");
        task_repo.delete(&task.id).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::claim_due_tasks;
    use super::finish_execution;
    use crate::storage::test_support::RecordingClient;
    use crate::storage::TaskDelivery;
    use crate::storage::TaskRecord;
    use crate::storage::TaskSchedule;

    fn sample_task(schedule_kind: &str, delete_after_run: bool) -> TaskRecord {
        TaskRecord {
            id: "task-1".to_string(),
            executor_node_id: "inst-1".to_string(),
            name: "nightly-report".to_string(),
            prompt: "run report".to_string(),
            enabled: true,
            status: "running".to_string(),
            schedule: match schedule_kind {
                "cron" => TaskSchedule::Cron {
                    expr: "0 0 9 * * *".to_string(),
                    tz: None,
                },
                "at" => TaskSchedule::At {
                    time: "2026-12-31T23:59:00Z".to_string(),
                },
                _ => TaskSchedule::Every { seconds: 60 },
            },
            delivery: TaskDelivery::None,
            last_error: None,
            delete_after_run,
            run_count: 0,
            last_run_at: String::new(),
            next_run_at: Some("2026-03-11 00:00:00".to_string()),
            lease_token: Some("lease-1".to_string()),
            lease_node_id: None,
            lease_expires_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[tokio::test]
    async fn claim_due_tasks_returns_rows_and_new_lease_token() {
        let claim_rows = Mutex::new(vec![vec![
            serde_json::Value::String("task-1".to_string()),
            serde_json::Value::String("inst-1".to_string()),
            serde_json::Value::String("nightly-report".to_string()),
            serde_json::Value::String("run report".to_string()),
            serde_json::Value::String("true".to_string()),
            serde_json::Value::String("running".to_string()),
            serde_json::Value::String("{\"kind\":\"every\",\"seconds\":60}".to_string()),
            serde_json::Value::String("{\"kind\":\"none\"}".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("false".to_string()),
            serde_json::Value::String("0".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("2026-03-11 00:00:00".to_string()),
            serde_json::Value::String("lease-1".to_string()),
            serde_json::Value::String(String::new()), // lease_node_id
            serde_json::Value::String(String::new()), // lease_expires_at
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
        ]]);
        let client = RecordingClient::new(move |sql, _database| {
            if sql.starts_with("SELECT id, executor_node_id, name, prompt, enabled, status, schedule, delivery, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, lease_node_id, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE lease_token = ") {
                return Ok(crate::storage::pool::QueryResponse {
                    id: String::new(),
                    state: "Succeeded".to_string(),
                    error: None,
                    data: claim_rows.lock().expect("claim rows lock").clone(),
                    next_uri: None,
                    final_uri: None,
                    schema: Vec::new(),
                });
            }
            Ok(crate::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: Vec::new(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        });
        let pool = client.pool();

        let (tasks, lease_token) = claim_due_tasks(&pool, "inst-1")
            .await
            .expect("claim due tasks");

        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, "task-1");
        assert!(!lease_token.is_empty());
        let sqls = client.sqls();
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("UPDATE tasks SET status = 'running'")));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("FROM tasks WHERE lease_token = ")));
    }

    #[tokio::test]
    async fn finish_execution_writes_history_and_updates_task() {
        let client = RecordingClient::new(|_sql, _database| {
            Ok(crate::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: Vec::new(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        });
        let pool = client.pool();
        let task = sample_task("every", false);

        finish_execution(
            &pool,
            &task,
            "lease-1",
            "inst-1",
            "ok",
            Some("run-1".to_string()),
            Some("done".to_string()),
            None,
            1200,
            Some("ok".to_string()),
            None,
        )
        .await
        .expect("finish execution");

        let sqls = client.sqls();
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("INSERT INTO task_history ")));
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("UPDATE tasks SET status = 'ok'")));
        assert!(!sqls
            .iter()
            .any(|sql| sql == "DELETE FROM tasks WHERE id = 'task-1'"));
    }

    #[tokio::test]
    async fn finish_execution_deletes_one_shot_after_run() {
        let client = RecordingClient::new(|_sql, _database| {
            Ok(crate::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: Vec::new(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        });
        let pool = client.pool();
        let task = sample_task("at", true);

        finish_execution(
            &pool,
            &task,
            "lease-1",
            "inst-1",
            "ok",
            Some("run-1".to_string()),
            Some("done".to_string()),
            None,
            1200,
            None,
            None,
        )
        .await
        .expect("finish execution");

        let sqls = client.sqls();
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("INSERT INTO task_history ")));
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("UPDATE tasks SET status = 'ok'")));
        assert!(sqls
            .iter()
            .any(|sql| sql == "DELETE FROM tasks WHERE id = 'task-1'"));
    }
}
