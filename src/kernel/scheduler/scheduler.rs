use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::executor;
use crate::kernel::runtime::Runtime;
use crate::kernel::task::execution;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 15;

pub struct TaskScheduler;

impl TaskScheduler {
    /// Spawn the background polling loop. Returns a JoinHandle that resolves
    /// when the loop exits (via cancellation or error).
    pub fn spawn(
        runtime: Arc<Runtime>,
        cancel: CancellationToken,
        http_client: reqwest::Client,
    ) -> JoinHandle<()> {
        let interval = Duration::from_secs(DEFAULT_POLL_INTERVAL_SECS);
        tokio::spawn(async move {
            tracing::info!("task scheduler started (poll interval: {interval:?})");
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("task scheduler shutting down");
                        return;
                    }
                    _ = tokio::time::sleep(interval) => {}
                }

                if let Err(e) = poll_once(&runtime, &http_client).await {
                    tracing::warn!(error = %e, "task scheduler poll error");
                }
            }
        })
    }
}

async fn poll_once(
    runtime: &Arc<Runtime>,
    http_client: &reqwest::Client,
) -> crate::base::Result<()> {
    let instance_id = runtime.config().instance_id.clone();
    let agent_ids = runtime.databases().list_agent_ids().await?;
    for agent_id in &agent_ids {
        let pool = match runtime.databases().agent_pool(agent_id) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(agent_id, error = %e, "failed to get agent pool");
                continue;
            }
        };

        let (claimed, lease_token) = match execution::claim_due_tasks(&pool, &instance_id).await {
            Ok(result) => result,
            Err(e) => {
                tracing::warn!(agent_id, error = %e, "failed to claim due tasks");
                continue;
            }
        };

        for task in claimed {
            let runtime = runtime.clone();
            let client = http_client.clone();
            let agent_id = agent_id.clone();
            let lease_token = lease_token.clone();
            tokio::spawn(async move {
                if let Err(e) =
                    executor::execute_task(&runtime, &agent_id, &task, &lease_token, &client).await
                {
                    tracing::error!(
                        agent_id,
                        task_id = task.id,
                        error = %e,
                        "task execution failed"
                    );
                }
            });
        }
    }
    Ok(())
}
