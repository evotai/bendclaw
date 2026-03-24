use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::kernel::channel::send_text_to_account;
use crate::kernel::channel::ChannelRegistry;
use crate::kernel::run::result::Reason;
use crate::kernel::runtime::Runtime;
use crate::kernel::session::session_stream::FinishedRunOutput;
use crate::kernel::task::execution;
use crate::observability::log::slog;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::Pool;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelDeliveryContext {
    pub channel_type: String,
    pub chat_id: String,
}

/// Execute a single claimed task: run prompt, deliver result,
/// then delegate to execution service for history + state update.
pub async fn execute_task(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    task: &TaskRecord,
    lease_token: &str,
    http_client: &reqwest::Client,
) -> crate::base::Result<()> {
    let pool = runtime.databases().agent_pool(agent_id)?;
    let node_id = runtime.config().node_id.clone();

    // 1. Execute the task prompt
    let started = Instant::now();
    let (status, run_id, output, error) = run_task_prompt(runtime, agent_id, task).await;
    let duration_ms = started.elapsed().as_millis() as i32;

    // 2. Delivery
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

    // 3. Delegate to execution service for history + state update
    execution::finish_execution(
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

    slog!(
        info,
        "task",
        "executed",
        agent_id,
        task_id = task.id,
        status,
        duration_ms,
        delivery_status = %delivery_status_log,
        delivery_error = %delivery_error_log,
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

    // Enrich prompt with channel delivery context so the LLM knows where to send.
    let prompt = enrich_prompt_with_delivery(&task.prompt, &task.delivery, runtime, agent_id).await;

    let stream = match session.run(&prompt, &task.id, None, "", "", false).await {
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

pub fn classify_task_run_output(
    finished: FinishedRunOutput,
) -> (String, Option<String>, Option<String>) {
    let text = if finished.text.trim().is_empty() {
        None
    } else {
        Some(finished.text)
    };

    match finished.stop_reason {
        Reason::EndTurn => ("ok".to_string(), text, None),
        reason => (
            "error".to_string(),
            text,
            Some(format!(
                "agent stopped before completing the task: {}",
                reason.as_str()
            )),
        ),
    }
}

/// If the task has channel delivery, append channel context to the prompt
/// so the LLM can use `channel_send` with the correct parameters.
async fn enrich_prompt_with_delivery(
    prompt: &str,
    delivery: &TaskDelivery,
    runtime: &Arc<Runtime>,
    agent_id: &str,
) -> String {
    match delivery {
        TaskDelivery::Channel {
            channel_account_id,
            chat_id,
        } => {
            match resolve_channel_delivery_context(runtime, agent_id, channel_account_id, chat_id)
                .await
            {
                Some(ctx) => format!(
                    "{prompt}\n\n\
                     [Channel context] When you need to send results, use channel_send with: \
                     channel_type=\"{}\", chat_id=\"{}\".",
                    ctx.channel_type, ctx.chat_id
                ),
                None => {
                    slog!(
                        warn,
                        "task",
                        "channel_context_unavailable",
                        agent_id,
                        channel_account_id,
                        chat_id,
                    );
                    format!(
                        "{prompt}\n\n\
                         [Channel context] Automatic delivery is configured by the system. \
                         If direct channel tools are unavailable, produce a final response and the system will deliver it."
                    )
                }
            }
        }
        _ => prompt.to_string(),
    }
}

pub async fn resolve_channel_delivery_context(
    runtime: &Arc<Runtime>,
    agent_id: &str,
    channel_account_id: &str,
    chat_id: &str,
) -> Option<ChannelDeliveryContext> {
    let pool = runtime.databases().agent_pool(agent_id).ok()?;
    let repo = ChannelAccountRepo::new(pool);
    let account = repo.load(channel_account_id).await.ok().flatten()?;
    if account.channel_type.trim().is_empty() {
        return None;
    }
    Some(ChannelDeliveryContext {
        channel_type: account.channel_type,
        chat_id: chat_id.to_string(),
    })
}

pub async fn deliver_result(
    channels: &ChannelRegistry,
    pool: &Pool,
    http_client: &reqwest::Client,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    match &task.delivery {
        TaskDelivery::None => (None, None),
        TaskDelivery::Webhook { url } => {
            deliver_webhook(http_client, url, task, status, output, error).await
        }
        TaskDelivery::Channel {
            channel_account_id,
            chat_id,
        } => {
            deliver_channel(
                channels,
                pool,
                channel_account_id,
                chat_id,
                task,
                status,
                output,
                error,
            )
            .await
        }
    }
}

pub async fn deliver_webhook(
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

#[allow(clippy::too_many_arguments)]
pub async fn deliver_channel(
    channels: &ChannelRegistry,
    pool: &Pool,
    channel_account_id: &str,
    chat_id: &str,
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> (Option<String>, Option<String>) {
    let repo = ChannelAccountRepo::new(pool.clone());
    let account = match repo.load(channel_account_id).await {
        Ok(Some(account)) => account,
        Ok(None) => {
            return (
                Some("failed".to_string()),
                Some(format!("channel account '{channel_account_id}' not found")),
            )
        }
        Err(e) => return (Some("failed".to_string()), Some(e.to_string())),
    };

    let text = render_delivery_text(task, status, output, error);
    match send_text_to_account(channels, &account, chat_id, &text).await {
        Ok(_) => (Some("ok".to_string()), None),
        Err(e) => (Some("failed".to_string()), Some(e.to_string())),
    }
}

pub fn render_delivery_text(
    task: &TaskRecord,
    status: &str,
    output: Option<&str>,
    error: Option<&str>,
) -> String {
    let mut sections = vec![format!(
        "Task '{}' finished with status '{}'.",
        task.name, status
    )];
    if let Some(output) = output.filter(|value| !value.trim().is_empty()) {
        sections.push(output.to_string());
    }
    if let Some(error) = error.filter(|value| !value.trim().is_empty()) {
        sections.push(format!("Error: {error}"));
    }
    sections.join("\n\n")
}

/// Compute the next run time based on schedule kind.
/// Kept as a public convenience wrapper around TaskSchedule.
pub fn compute_next_run(schedule: &TaskSchedule) -> Option<String> {
    schedule.next_run_at()
}
