use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::kernel::runtime::Runtime;
use crate::kernel::task::execution;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskSchedule;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

/// Execute a single claimed task: run prompt, deliver webhook,
/// then delegate to execution service for history + state update.
pub async fn execute_task(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    task: &TaskRecord,
    lease_token: &str,
    http_client: &reqwest::Client,
) -> crate::base::Result<()> {
    let pool = runtime.databases().agent_pool(agent_id)?;
    let executor_instance_id = runtime.config().instance_id.clone();

    // 1. Execute the task prompt
    let started = Instant::now();
    let (status, run_id, output, error) = run_task_prompt(runtime, agent_id, task).await;
    let duration_ms = started.elapsed().as_millis() as i32;

    // 2. Webhook delivery
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

    // 3. Delegate to execution service for history + state update
    execution::finish_execution(
        &pool,
        task,
        lease_token,
        &executor_instance_id,
        &status,
        run_id,
        output,
        error,
        duration_ms,
        webhook_status,
        webhook_error,
    )
    .await?;

    tracing::info!(
        agent_id,
        task_id = task.id,
        status,
        duration_ms,
        "task executed"
    );
    Ok(())
}

async fn run_task_prompt(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    task: &TaskRecord,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let session_id = format!("task_{}", task.id);
    let session = match runtime
        .get_or_create_session(agent_id, &session_id, "system")
        .await
    {
        Ok(s) => s,
        Err(e) => {
            return (
                "error".to_string(),
                None,
                None,
                Some(format!("failed to create session: {e}")),
            )
        }
    };
    let stream = match session.run(&task.prompt, &task.id, None).await {
        Ok(s) => s,
        Err(e) => {
            return (
                "error".to_string(),
                None,
                None,
                Some(format!("failed to start run: {e}")),
            )
        }
    };
    let run_id = stream.run_id().to_string();
    match stream.finish().await {
        Ok(output) => ("ok".to_string(), Some(run_id), Some(output), None),
        Err(e) => ("error".to_string(), Some(run_id), None, Some(e.to_string())),
    }
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
/// Kept as a public convenience wrapper around TaskSchedule.
pub fn compute_next_run(
    schedule_kind: &str,
    cron_expr: &str,
    every_seconds: Option<i32>,
) -> Option<String> {
    let schedule = TaskSchedule::from_record(schedule_kind, cron_expr, every_seconds, None, None)?;
    schedule.next_run_at()
}
