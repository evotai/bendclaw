use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::executor;
use crate::storage::AgentDatabases;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 15;

pub struct TaskScheduler;

impl TaskScheduler {
    /// Spawn the background polling loop. Returns a JoinHandle that resolves
    /// when the loop exits (via cancellation or error).
    pub fn spawn(
        databases: Arc<AgentDatabases>,
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

                if let Err(e) = poll_once(&databases, &http_client).await {
                    tracing::warn!(error = %e, "task scheduler poll error");
                }
            }
        })
    }
}

async fn poll_once(
    databases: &AgentDatabases,
    http_client: &reqwest::Client,
) -> crate::base::Result<()> {
    let agent_ids = databases.list_agent_ids().await?;
    for agent_id in &agent_ids {
        let pool = match databases.agent_pool(agent_id) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(agent_id, error = %e, "failed to get agent pool");
                continue;
            }
        };

        let task_repo = crate::storage::dal::task::TaskRepo::new(pool.clone());
        let due_tasks = match task_repo.list_due().await {
            Ok(tasks) => tasks,
            Err(e) => {
                tracing::warn!(agent_id, error = %e, "failed to list due tasks");
                continue;
            }
        };

        for task in due_tasks {
            let pool = pool.clone();
            let client = http_client.clone();
            let agent_id = agent_id.clone();
            tokio::spawn(async move {
                if let Err(e) = executor::execute_task(&pool, &agent_id, &task, &client).await {
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
