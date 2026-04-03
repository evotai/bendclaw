use std::sync::Arc;
use std::time::Instant;

use super::finish_execution::finish_execution;
use super::prompt_builder::enrich_prompt_with_delivery;
use super::task_result::classify_task_run_output;
use crate::runtime::Runtime;
use crate::storage::dal::task::TaskRecord;
use crate::tasks::delivery::delivery_service::deliver_result;
use crate::tasks::diagnostics;

/// Execute a single claimed task: run prompt, deliver result,
/// then delegate to finish_execution for history + state update.
pub async fn execute_task(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    task: &TaskRecord,
    lease_token: &str,
    http_client: &reqwest::Client,
) -> crate::types::Result<()> {
    let pool = runtime.databases().agent_pool(agent_id)?;
    let node_id = runtime.config().node_id.clone();

    let started = Instant::now();
    let (status, run_id, output, error) = run_task_prompt(runtime, agent_id, task).await;
    let duration_ms = started.elapsed().as_millis() as i32;

    let (delivery_status, delivery_error) = deliver_result(
        runtime.channels().as_ref(),
        &pool,
        http_client,
        task,
        &status,
        output.as_deref(),
        error.as_deref(),
    )
    .await;
    let delivery_status_log = delivery_status.as_deref().unwrap_or("n/a").to_string();
    let delivery_error_log = delivery_error.as_deref().unwrap_or("").to_string();

    finish_execution(
        &pool,
        task,
        lease_token,
        &node_id,
        &status,
        run_id,
        output,
        error,
        duration_ms,
        delivery_status,
        delivery_error,
    )
    .await?;

    diagnostics::log_task_executed(
        agent_id,
        &task.id,
        &status,
        duration_ms,
        &delivery_status_log,
        &delivery_error_log,
    );
    Ok(())
}

async fn run_task_prompt(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    task: &TaskRecord,
) -> (String, Option<String>, Option<String>, Option<String>) {
    let session_id = format!("task_{}", task.id);
    let session = match crate::sessions::factory::acquire_cloud_session(
        runtime,
        agent_id,
        &session_id,
        "system",
    )
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

    let prompt = enrich_prompt_with_delivery(&task.prompt, &task.delivery, runtime, agent_id).await;

    let stream = match session
        .submit_turn(&prompt, &task.id, None, "", "", false)
        .await
    {
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
    match stream.finish_output().await {
        Ok(finished) => {
            let (status, output, error) = classify_task_run_output(finished);
            (status, Some(run_id), output, error)
        }
        Err(e) => ("error".to_string(), Some(run_id), None, Some(e.to_string())),
    }
}
