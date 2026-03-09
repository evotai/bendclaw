use std::str::FromStr;
use std::time::Duration;
use std::time::Instant;

use chrono::Utc;
use cron::Schedule;

use crate::base::new_id;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task_history::TaskHistoryRecord;
use crate::storage::dal::task_history::TaskHistoryRepo;
use crate::storage::pool::Pool;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Execute a single due task: mark running, run prompt, write history,
/// call webhook, update task state, and optionally delete if one-shot.
pub async fn execute_task(
    pool: &Pool,
    agent_id: &str,
    task: &TaskRecord,
    http_client: &reqwest::Client,
) -> crate::base::Result<()> {
    let task_repo = TaskRepo::new(pool.clone());
    let history_repo = TaskHistoryRepo::new(pool.clone());

    // 1. Mark as running
    task_repo.set_running(&task.id).await?;

    // 2. Execute the task prompt
    let started = Instant::now();
    let (status, output, error) = run_task_prompt(agent_id, task).await;
    let duration_ms = started.elapsed().as_millis() as i32;

    // 3. Webhook delivery
    let (webhook_status, webhook_error) = if let Some(url) = &task.webhook_url {
        deliver_webhook(
            http_client,
            url,
            task,
            &status,
            output.as_deref(),
            error.as_deref(),
        )
        .await
    } else {
        (None, None)
    };

    // 4. Write history snapshot
    let history = TaskHistoryRecord {
        id: new_id(),
        task_id: task.id.clone(),
        run_id: None,
        task_name: task.name.clone(),
        schedule_kind: task.schedule_kind.clone(),
        cron_expr: if task.cron_expr.is_empty() {
            None
        } else {
            Some(task.cron_expr.clone())
        },
        prompt: task.prompt.clone(),
        status: status.clone(),
        output: output.clone(),
        error: error.clone(),
        duration_ms: Some(duration_ms),
        webhook_url: task.webhook_url.clone(),
        webhook_status,
        webhook_error,
        created_at: String::new(),
    };
    if let Err(e) = history_repo.insert(&history).await {
        tracing::error!(task_id = task.id, error = %e, "failed to write task history");
    }

    // 5. Compute next_run_at
    let next_run_at = compute_next_run(&task.schedule_kind, &task.cron_expr, task.every_seconds);

    // 6. Update task state
    task_repo
        .update_after_run(&task.id, &status, error.as_deref(), next_run_at.as_deref())
        .await?;

    // 7. Auto-delete one-shot tasks
    if task.delete_after_run && task.schedule_kind == "at" {
        tracing::info!(task_id = task.id, "deleting one-shot task after run");
        task_repo.delete(&task.id).await?;
    }

    tracing::info!(
        agent_id,
        task_id = task.id,
        status,
        duration_ms,
        "task executed"
    );
    Ok(())
}

/// Placeholder for actual prompt execution. In the future this will create
/// a run via the kernel engine. For now it returns a successful no-op.
async fn run_task_prompt(
    _agent_id: &str,
    _task: &TaskRecord,
) -> (String, Option<String>, Option<String>) {
    // TODO: integrate with kernel run engine to actually execute the prompt
    ("ok".to_string(), None, None)
}

async fn deliver_webhook(
    client: &reqwest::Client,
    url: &str,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    let payload = serde_json::json!({
        "task_id": task.id,
        "task_name": task.name,
        "status": status,
        "output": output,
        "error": error,
    });

    match client
        .post(url)
        .timeout(WEBHOOK_TIMEOUT)
        .json(&payload)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => (Some("ok".to_string()), None),
        Ok(resp) => (
            Some("failed".to_string()),
            Some(format!("HTTP {}", resp.status())),
        ),
        Err(e) => (Some("failed".to_string()), Some(e.to_string())),
    }
}

/// Compute the next run time based on schedule kind.
/// Returns a concrete UTC timestamp string (e.g. "2026-03-09T10:00:00Z").
pub fn compute_next_run(
    schedule_kind: &str,
    cron_expr: &str,
    every_seconds: Option<i32>,
) -> Option<String> {
    match schedule_kind {
        "every" => {
            let secs = every_seconds.unwrap_or(60) as i64;
            let next = Utc::now() + chrono::Duration::seconds(secs);
            Some(next.format("%Y-%m-%d %H:%M:%S").to_string())
        }
        "at" => None, // one-shot, no next run
        "cron" => {
            if cron_expr.is_empty() {
                return None;
            }
            match Schedule::from_str(cron_expr) {
                Ok(schedule) => schedule
                    .upcoming(Utc)
                    .next()
                    .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()),
                Err(e) => {
                    tracing::warn!(cron_expr, error = %e, "invalid cron expression");
                    None
                }
            }
        }
        _ => None,
    }
}
