use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use tokio::sync::Semaphore;

use super::executor;
use crate::kernel::runtime::Runtime;
use crate::kernel::task::execution;

const DEFAULT_POLL_INTERVAL_SECS: u64 = 30;
const MAX_CONCURRENT_TASKS: usize = 32;

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
        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));
        tokio::spawn(async move {
            tracing::info!("task scheduler started (poll interval: {interval:?})");
            let mut consecutive_errors: u64 = 0;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        tracing::info!("task scheduler shutting down");
                        return;
                    }
                    _ = tokio::time::sleep(interval) => {}
                }

                if let Err(e) = poll_once(&runtime, &http_client, &sem).await {
                    consecutive_errors += 1;
                    // Log first failure, then every 20th to avoid flooding.
                    if consecutive_errors == 1 || consecutive_errors.is_multiple_of(20) {
                        tracing::warn!(
                            error = %e,
                            consecutive_errors,
                            "task scheduler poll error"
                        );
                    }
                } else {
                    if consecutive_errors > 0 {
                        tracing::info!(consecutive_errors, "task scheduler recovered");
                    }
                    consecutive_errors = 0;
                }
            }
        })
    }
}

async fn poll_once(
    runtime: &Arc<Runtime>,
    http_client: &reqwest::Client,
    sem: &Arc<Semaphore>,
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
            let guard = runtime.track_task();
            let sem = sem.clone();
            tokio::spawn(async move {
                let _permit = match sem.acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => return, // semaphore closed
                };
                let _guard = guard;
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use parking_lot::RwLock;

    use super::poll_once;
    use super::TaskScheduler;
    use super::MAX_CONCURRENT_TASKS;
    use tokio::sync::Semaphore;
    use crate::base::ErrorCode;
    use crate::kernel::channel::registry::ChannelRegistry;
    use crate::kernel::channel::supervisor::ChannelSupervisor;
    use crate::kernel::runtime::agent_config::AgentConfig;
    use crate::kernel::runtime::runtime::RuntimeParts;
    use crate::kernel::runtime::Runtime;
    use crate::kernel::runtime::RuntimeStatus;
    use crate::kernel::session::SessionManager;
    use crate::kernel::skills::store::SkillStore;
    use crate::llm::message::ChatMessage;
    use crate::llm::provider::LLMProvider;
    use crate::llm::provider::LLMResponse;
    use crate::llm::stream::ResponseStream;
    use crate::llm::tool::ToolSchema;
    use crate::storage::test_support::RecordingClient;
    use crate::storage::AgentDatabases;

    struct NoopLLM;

    #[async_trait]
    impl LLMProvider for NoopLLM {
        async fn chat(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            _tools: &[ToolSchema],
            _temperature: f32,
        ) -> crate::base::Result<LLMResponse> {
            Err(ErrorCode::internal("noop llm"))
        }

        fn chat_stream(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            _tools: &[ToolSchema],
            _temperature: f32,
        ) -> ResponseStream {
            let (_writer, stream) = ResponseStream::channel(1);
            stream
        }
    }

    fn runtime_with_client(client: &RecordingClient) -> Arc<Runtime> {
        let pool = client.pool();
        let databases = Arc::new(AgentDatabases::new(pool, "test_").expect("agent databases"));
        let workspace_root =
            std::env::temp_dir().join(format!("bendclaw-scheduler-{}", ulid::Ulid::new()));
        let _ = std::fs::create_dir_all(&workspace_root);
        let skills = Arc::new(SkillStore::new(databases.clone(), workspace_root, None));
        let channels = Arc::new(ChannelRegistry::new());
        let supervisor = Arc::new(ChannelSupervisor::new(
            channels.clone(),
            Arc::new(|_, _| {}),
        ));

        let config = AgentConfig {
            instance_id: "inst-1".to_string(),
            ..AgentConfig::default()
        };

        Arc::new(Runtime::from_parts(RuntimeParts {
            config,
            databases,
            llm: RwLock::new(Arc::new(NoopLLM)),
            agent_llms: RwLock::new(HashMap::new()),
            skills,
            sessions: Arc::new(SessionManager::new()),
            channels,
            supervisor,
            status: RwLock::new(RuntimeStatus::Ready),
            sync_cancel: tokio_util::sync::CancellationToken::new(),
            sync_handle: RwLock::new(None),
            scheduler_handle: RwLock::new(None),
            cluster: None,
            heartbeat_handle: RwLock::new(None),
            directive: None,
            directive_handle: RwLock::new(None),
            activity_tracker: Arc::new(crate::kernel::runtime::ActivityTracker::new()),
        }))
    }

    #[tokio::test]
    async fn poll_once_handles_no_agent_databases() {
        let client = RecordingClient::new(|sql, _database| {
            if sql.starts_with("SHOW DATABASES LIKE ") {
                return Ok(crate::storage::pool::QueryResponse {
                    id: String::new(),
                    state: "Succeeded".to_string(),
                    error: None,
                    data: Vec::new(),
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
        let runtime = runtime_with_client(&client);

        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));
        poll_once(&runtime, &reqwest::Client::new(), &sem)
            .await
            .expect("poll without agents");

        let sqls = client.sqls();
        assert_eq!(sqls.len(), 1);
        assert!(sqls[0].starts_with("SHOW DATABASES LIKE 'test_%'"));
    }

    #[tokio::test]
    async fn poll_once_claims_due_tasks_for_each_agent() {
        let database_rows = vec![vec![serde_json::Value::String("test_agent-a".to_string())]];
        let due_rows = Vec::<Vec<serde_json::Value>>::new();
        let client = RecordingClient::new(move |sql, _database| {
            if sql.starts_with("SHOW DATABASES LIKE ") {
                return Ok(crate::storage::pool::QueryResponse {
                    id: String::new(),
                    state: "Succeeded".to_string(),
                    error: None,
                    data: database_rows.clone(),
                    next_uri: None,
                    final_uri: None,
                    schema: Vec::new(),
                });
            }
            if sql.starts_with("SELECT id, executor_instance_id, name, prompt, enabled, status, schedule, delivery, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE lease_token = ") {
                return Ok(crate::storage::pool::QueryResponse {
                    id: String::new(),
                    state: "Succeeded".to_string(),
                    error: None,
                    data: due_rows.clone(),
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
        let runtime = runtime_with_client(&client);

        let sem = Arc::new(Semaphore::new(MAX_CONCURRENT_TASKS));
        poll_once(&runtime, &reqwest::Client::new(), &sem)
            .await
            .expect("poll with one agent");

        let sqls = client.sqls();
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("SHOW DATABASES LIKE 'test_%'")));
        assert!(sqls
            .iter()
            .any(|sql| sql.starts_with("UPDATE tasks SET status = 'running'")));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("FROM tasks WHERE lease_token = ")));
    }

    #[tokio::test]
    async fn scheduler_spawn_exits_when_cancelled() {
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
        let runtime = runtime_with_client(&client);
        let cancel = tokio_util::sync::CancellationToken::new();
        let handle = TaskScheduler::spawn(runtime, cancel.clone(), reqwest::Client::new());
        cancel.cancel();
        handle.await.expect("scheduler join");
    }
}
