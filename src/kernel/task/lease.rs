use std::sync::Arc;

use async_trait::async_trait;

use crate::base::Result;
use crate::kernel::lease::types::LeaseResource;
use crate::kernel::lease::types::ReleaseFn;
use crate::kernel::lease::types::ResourceEntry;
use crate::kernel::runtime::Runtime;
use crate::storage::dal::task::repo::TaskRepo;
use crate::storage::pool::Pool;

/// Drop guard that calls release_fn when the spawned task exits,
/// regardless of whether it completed normally or panicked.
struct LeaseGuard {
    task_id: String,
    release_fn: Option<ReleaseFn>,
}

impl Drop for LeaseGuard {
    fn drop(&mut self) {
        if let Some(release) = self.release_fn.take() {
            release(&self.task_id);
        }
    }
}

pub struct TaskLeaseResource {
    runtime: Arc<Runtime>,
    http_client: reqwest::Client,
}

impl TaskLeaseResource {
    pub fn new(runtime: Arc<Runtime>, http_client: reqwest::Client) -> Self {
        Self {
            runtime,
            http_client,
        }
    }
}

#[async_trait]
impl LeaseResource for TaskLeaseResource {
    fn table(&self) -> &str {
        "tasks"
    }

    fn lease_secs(&self) -> u64 {
        300
    }

    fn scan_interval_secs(&self) -> u64 {
        30
    }

    fn claim_condition(&self) -> Option<&str> {
        Some(
            "enabled = true AND next_run_at <= NOW() AND (\
                status != 'running' \
                OR (status = 'running' AND (lease_expires_at IS NULL OR lease_expires_at <= NOW()))\
            )",
        )
    }

    async fn discover(&self) -> Result<Vec<ResourceEntry>> {
        let agent_ids = self.runtime.databases().list_agent_ids().await?;
        let mut entries = Vec::new();

        for agent_id in &agent_ids {
            let pool = match self.runtime.databases().agent_pool(agent_id) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(agent_id, error = %e, "skip agent for task lease discover");
                    continue;
                }
            };

            let repo = TaskRepo::new(pool.clone());
            let tasks = match repo.list_active().await {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!(agent_id, error = %e, "failed to list due tasks");
                    continue;
                }
            };

            for task in tasks {
                entries.push(ResourceEntry {
                    id: task.id.clone(),
                    pool: pool.clone(),
                    lease_token: task.lease_token,
                    lease_instance_id: task.lease_instance_id,
                    lease_expires_at: task.lease_expires_at,
                    context: agent_id.clone(),
                    release_fn: None,
                });
            }
        }

        Ok(entries)
    }

    async fn on_acquired(&self, entry: &ResourceEntry) -> Result<()> {
        let repo = TaskRepo::new(entry.pool.clone());
        let task = repo.get(&entry.id).await?.ok_or_else(|| {
            crate::base::ErrorCode::internal(format!("task '{}' disappeared after claim", entry.id))
        })?;

        let agent_id = if entry.context.is_empty() {
            return Err(crate::base::ErrorCode::internal(format!(
                "no agent_id context for task '{}'",
                entry.id
            )));
        } else {
            entry.context.clone()
        };

        // Set status='running' — owned by the task domain layer.
        repo.set_status_running(&entry.id).await?;

        let lease_token = entry.lease_token.clone().unwrap_or_default();
        let release_fn = entry.release_fn.clone();
        let task_id = entry.id.clone();
        let runtime = self.runtime.clone();
        let client = self.http_client.clone();
        let guard = runtime.track_task();

        tokio::spawn(async move {
            let _guard = guard;
            let _lease_guard = LeaseGuard {
                task_id,
                release_fn,
            };
            if let Err(e) =
                super::executor::execute_task(&runtime, &agent_id, &task, &lease_token, &client)
                    .await
            {
                tracing::error!(
                    task_id = task.id,
                    agent_id,
                    error = %e,
                    "task execution failed"
                );
            }
        });

        Ok(())
    }

    async fn on_released(&self, resource_id: &str, pool: &Pool) {
        let repo = TaskRepo::new(pool.clone());
        if let Err(e) = repo.reset_status_if_running(resource_id).await {
            tracing::warn!(
                task_id = %resource_id,
                error = %e,
                "failed to reset task status on lease release"
            );
        }
    }

    fn safe_to_release(&self) -> bool {
        self.runtime.activity_tracker.active_task_count() == 0
    }
}
