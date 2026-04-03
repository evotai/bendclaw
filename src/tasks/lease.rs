use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;

use crate::lease::types::LeaseResource;
use crate::lease::types::ReleaseFn;
use crate::lease::types::ResourceEntry;
use crate::runtime::Runtime;
use crate::storage::dal::task::repo::TaskRepo;
use crate::storage::pool::Pool;
use crate::tasks::diagnostics;
use crate::types::Result;

/// Maximum wall-clock time a single task execution may take.
/// Prevents runaway LLM calls or hung deliveries from holding a lease forever.
const TASK_EXECUTION_TIMEOUT: Duration = Duration::from_secs(270);

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
        60
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
                    diagnostics::log_task_discover_skip(agent_id, &e);
                    continue;
                }
            };

            let repo = TaskRepo::new(pool.clone());
            let tasks = match repo.list_active().await {
                Ok(t) => t,
                Err(e) => {
                    diagnostics::log_task_list_failed(agent_id, &e);
                    continue;
                }
            };

            for task in tasks {
                entries.push(ResourceEntry {
                    id: task.id.clone(),
                    pool: pool.clone(),
                    lease_token: task.lease_token,
                    lease_node_id: task.lease_node_id,
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
            crate::types::ErrorCode::internal(format!(
                "task '{}' disappeared after claim",
                entry.id
            ))
        })?;

        let agent_id = if entry.context.is_empty() {
            return Err(crate::types::ErrorCode::internal(format!(
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

        crate::types::spawn_fire_and_forget("task_execution", async move {
            let _guard = guard;
            let _lease_guard = LeaseGuard {
                task_id,
                release_fn,
            };
            let result = tokio::time::timeout(
                TASK_EXECUTION_TIMEOUT,
                super::execution::execute_task(&runtime, &agent_id, &task, &lease_token, &client),
            )
            .await;
            match result {
                Ok(Err(e)) => {
                    diagnostics::log_task_execution_failed(&task.id, &agent_id, &e);
                }
                Err(_) => {
                    diagnostics::log_task_execution_timeout(
                        &task.id,
                        &agent_id,
                        TASK_EXECUTION_TIMEOUT.as_secs(),
                    );
                }
                Ok(Ok(())) => {}
            }
        });

        Ok(())
    }

    async fn on_released(&self, resource_id: &str, pool: &Pool) {
        let repo = TaskRepo::new(pool.clone());
        if let Err(e) = repo.reset_status_if_running(resource_id).await {
            diagnostics::log_task_reset_status_failed(resource_id, &e);
        }
    }

    fn safe_to_release(&self) -> bool {
        self.runtime.activity_tracker.active_task_count() == 0
    }
}
