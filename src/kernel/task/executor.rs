use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crate::kernel::channel::send_text_to_account;
use crate::kernel::channel::ChannelRegistry;
use crate::kernel::runtime::Runtime;
use crate::kernel::task::execution;
use crate::storage::dal::channel_account::repo::ChannelAccountRepo;
use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::Pool;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(10);

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
    let executor_node_id = runtime.config().node_id.clone();

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

    // 3. Delegate to execution service for history + state update
    execution::finish_execution(
        &pool,
        task,
        lease_token,
        &executor_node_id,
        &status,
        run_id,
        output,
        error,
        duration_ms,
        delivery_status,
        delivery_error,
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
    let stream = match session
        .run(&task.prompt, &task.id, None, "", "", false)
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
    match stream.finish().await {
        Ok(output) => ("ok".to_string(), Some(run_id), Some(output), None),
        Err(e) => ("error".to_string(), Some(run_id), None, Some(e.to_string())),
    }
}

async fn deliver_result(
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

#[allow(clippy::too_many_arguments)]
async fn deliver_channel(
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

fn render_delivery_text(
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use anyhow::Result;
    use async_trait::async_trait;
    use axum::routing::post;
    use axum::Json;
    use axum::Router;
    use chrono::NaiveDateTime;
    use chrono::Utc;
    use tokio_util::sync::CancellationToken;

    use super::compute_next_run;
    use super::deliver_channel;
    use super::deliver_result;
    use super::deliver_webhook;
    use super::render_delivery_text;
    use crate::base::Result as BaseResult;
    use crate::kernel::channel::account::ChannelAccount;
    use crate::kernel::channel::capabilities::ChannelCapabilities;
    use crate::kernel::channel::capabilities::ChannelKind;
    use crate::kernel::channel::capabilities::InboundMode;
    use crate::kernel::channel::plugin::ChannelOutbound;
    use crate::kernel::channel::plugin::ChannelPlugin;
    use crate::kernel::channel::plugin::InboundEventSender;
    use crate::kernel::channel::plugin::InboundKind;
    use crate::kernel::channel::plugin::ReceiverFactory;
    use crate::kernel::channel::ChannelRegistry;
    use crate::storage::test_support::RecordingClient;
    use crate::storage::TaskDelivery;
    use crate::storage::TaskRecord;
    use crate::storage::TaskSchedule;

    fn sample_task() -> TaskRecord {
        TaskRecord {
            id: "task-1".to_string(),
            executor_node_id: "inst-1".to_string(),
            name: "nightly-report".to_string(),
            prompt: "run report".to_string(),
            enabled: true,
            status: "idle".to_string(),
            schedule: TaskSchedule::Every { seconds: 60 },
            delivery: TaskDelivery::None,
            last_error: None,
            delete_after_run: false,
            run_count: 0,
            last_run_at: String::new(),
            next_run_at: None,
            lease_token: None,
            lease_node_id: None,
            lease_expires_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn fake_pool(rows: Vec<Vec<serde_json::Value>>) -> crate::storage::Pool {
        let rows = Arc::new(Mutex::new(rows));
        RecordingClient::new(move |_sql, _database| {
            Ok(crate::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: rows.lock().expect("channel rows").clone(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        })
        .pool()
    }

    #[derive(Clone, Default)]
    struct RecordingPlugin {
        sent: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl ChannelPlugin for RecordingPlugin {
        fn channel_type(&self) -> &str {
            "test"
        }

        fn capabilities(&self) -> ChannelCapabilities {
            ChannelCapabilities {
                channel_kind: ChannelKind::Conversational,
                inbound_mode: InboundMode::WebSocket,
                supports_edit: false,
                supports_streaming: false,
                supports_markdown: true,
                supports_threads: false,
                supports_reactions: false,
                max_message_len: 4096,
            }
        }

        fn validate_config(&self, _config: &serde_json::Value) -> BaseResult<()> {
            Ok(())
        }

        fn outbound(&self) -> Arc<dyn ChannelOutbound> {
            Arc::new(RecordingOutbound {
                sent: Arc::clone(&self.sent),
            })
        }

        fn inbound(&self) -> InboundKind {
            InboundKind::Receiver(Arc::new(NoopReceiverFactory))
        }
    }

    struct NoopReceiverFactory;

    #[async_trait]
    impl ReceiverFactory for NoopReceiverFactory {
        async fn spawn(
            &self,
            _account: &ChannelAccount,
            _event_tx: InboundEventSender,
            _cancel: CancellationToken,
        ) -> BaseResult<tokio::task::JoinHandle<()>> {
            Ok(tokio::spawn(async {}))
        }
    }

    struct RecordingOutbound {
        sent: Arc<Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl ChannelOutbound for RecordingOutbound {
        async fn send_text(
            &self,
            _config: &serde_json::Value,
            chat_id: &str,
            text: &str,
        ) -> BaseResult<String> {
            self.sent
                .lock()
                .expect("sent messages")
                .push((chat_id.to_string(), text.to_string()));
            Ok("msg-1".to_string())
        }

        async fn send_typing(&self, _config: &serde_json::Value, _chat_id: &str) -> BaseResult<()> {
            Ok(())
        }

        async fn edit_message(
            &self,
            _config: &serde_json::Value,
            _chat_id: &str,
            _msg_id: &str,
            _text: &str,
        ) -> BaseResult<()> {
            Ok(())
        }

        async fn add_reaction(
            &self,
            _config: &serde_json::Value,
            _chat_id: &str,
            _msg_id: &str,
            _emoji: &str,
        ) -> BaseResult<()> {
            Ok(())
        }
    }

    fn channel_account_row(id: &str, enabled: bool) -> Vec<serde_json::Value> {
        vec![
            serde_json::Value::String(id.to_string()),
            serde_json::Value::String("test".to_string()),
            serde_json::Value::String("account-1".to_string()),
            serde_json::Value::String("agent-1".to_string()),
            serde_json::Value::String("user-1".to_string()),
            serde_json::Value::String("{}".to_string()),
            serde_json::Value::String(if enabled { "1" } else { "0" }.to_string()),
            serde_json::Value::String("".to_string()), // lease_node_id
            serde_json::Value::String("".to_string()), // lease_token
            serde_json::Value::String("".to_string()), // lease_expires_at
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-10T00:00:00Z".to_string()),
        ]
    }

    async fn start_webhook_server(status: axum::http::StatusCode) -> String {
        async fn ok(
            Json(payload): Json<serde_json::Value>,
        ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
            (axum::http::StatusCode::OK, Json(payload))
        }

        async fn fail(
            Json(payload): Json<serde_json::Value>,
        ) -> (axum::http::StatusCode, Json<serde_json::Value>) {
            (axum::http::StatusCode::BAD_GATEWAY, Json(payload))
        }

        let app = match status {
            axum::http::StatusCode::OK => Router::new().route("/", post(ok)),
            _ => Router::new().route("/", post(fail)),
        };
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind webhook server");
        let addr = listener.local_addr().expect("webhook addr");
        tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve webhook server");
        });
        format!("http://{addr}/")
    }

    #[test]
    fn compute_next_run_every_returns_future_timestamp() -> Result<()> {
        let before = Utc::now();
        let next = compute_next_run(&TaskSchedule::Every { seconds: 30 }).expect("next run");
        let parsed = NaiveDateTime::parse_from_str(&next, "%Y-%m-%d %H:%M:%S")?;
        let diff = parsed.and_utc() - before;
        assert!(diff.num_seconds() >= 29 && diff.num_seconds() <= 31);
        Ok(())
    }

    #[test]
    fn compute_next_run_at_returns_none() {
        assert!(compute_next_run(&TaskSchedule::At {
            time: "2026-12-31T23:59:00Z".to_string()
        })
        .is_none());
    }

    #[test]
    fn compute_next_run_invalid_schedule_returns_none() {
        assert!(TaskSchedule::Cron {
            expr: String::new(),
            tz: None
        }
        .next_run_at()
        .is_none());
    }

    #[tokio::test]
    async fn deliver_webhook_reports_success() {
        let client = reqwest::Client::new();
        let url = start_webhook_server(axum::http::StatusCode::OK).await;
        let task = sample_task();

        let (status, error) = deliver_webhook(&client, &url, &task, "ok", Some("done"), None).await;

        assert_eq!(status.as_deref(), Some("ok"));
        assert!(error.is_none());
    }

    #[tokio::test]
    async fn deliver_webhook_reports_http_failure() {
        let client = reqwest::Client::new();
        let url = start_webhook_server(axum::http::StatusCode::BAD_GATEWAY).await;
        let task = sample_task();

        let (status, error) =
            deliver_webhook(&client, &url, &task, "error", None, Some("boom")).await;

        assert_eq!(status.as_deref(), Some("failed"));
        assert!(error.as_deref().is_some_and(|value| value.contains("502")));
    }

    #[test]
    fn render_delivery_text_includes_output_and_error() {
        let text = render_delivery_text(&sample_task(), "error", Some("partial"), Some("boom"));
        assert!(text.contains("nightly-report"));
        assert!(text.contains("partial"));
        assert!(text.contains("Error: boom"));
    }

    #[tokio::test]
    async fn deliver_channel_reports_success() {
        let mut registry = ChannelRegistry::new();
        let plugin = Arc::new(RecordingPlugin::default());
        registry.register(plugin.clone());
        let pool = fake_pool(vec![channel_account_row("channel-1", true)]);
        let task = TaskRecord {
            delivery: TaskDelivery::Channel {
                channel_account_id: "channel-1".to_string(),
                chat_id: "chat-42".to_string(),
            },
            ..sample_task()
        };

        let (status, error) = deliver_channel(
            &registry,
            &pool,
            "channel-1",
            "chat-42",
            &task,
            "ok",
            Some("done"),
            None,
        )
        .await;

        assert_eq!(status.as_deref(), Some("ok"));
        assert!(error.is_none());
        assert_eq!(plugin.sent.lock().expect("sent messages").clone(), vec![(
            "chat-42".to_string(),
            "Task 'nightly-report' finished with status 'ok'.\n\ndone".to_string()
        )]);
    }

    #[tokio::test]
    async fn deliver_result_reports_missing_channel_account() {
        let registry = ChannelRegistry::new();
        let pool = fake_pool(Vec::new());
        let task = TaskRecord {
            delivery: TaskDelivery::Channel {
                channel_account_id: "missing".to_string(),
                chat_id: "chat-42".to_string(),
            },
            ..sample_task()
        };

        let (status, error) = deliver_result(
            &registry,
            &pool,
            &reqwest::Client::new(),
            &task,
            "ok",
            Some("done"),
            None,
        )
        .await;

        assert_eq!(status.as_deref(), Some("failed"));
        assert!(error
            .as_deref()
            .is_some_and(|value| value.contains("not found")));
    }
}
