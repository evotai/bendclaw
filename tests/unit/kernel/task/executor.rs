use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use axum::routing::post;
use axum::Json;
use axum::Router;
use bendclaw::base::Result as BaseResult;
use bendclaw::kernel::channel::account::ChannelAccount;
use bendclaw::kernel::channel::capabilities::ChannelCapabilities;
use bendclaw::kernel::channel::capabilities::ChannelKind;
use bendclaw::kernel::channel::capabilities::InboundMode;
use bendclaw::kernel::channel::plugin::ChannelOutbound;
use bendclaw::kernel::channel::plugin::ChannelPlugin;
use bendclaw::kernel::channel::plugin::InboundEventSender;
use bendclaw::kernel::channel::plugin::InboundKind;
use bendclaw::kernel::channel::plugin::ReceiverFactory;
use bendclaw::kernel::channel::ChannelRegistry;
use bendclaw::kernel::run::result::Reason;
use bendclaw::kernel::session::session_stream::FinishedRunOutput;
use bendclaw::kernel::task::executor::classify_task_run_output;
use bendclaw::kernel::task::executor::compute_next_run;
use bendclaw::kernel::task::executor::deliver_channel;
use bendclaw::kernel::task::executor::deliver_result;
use bendclaw::kernel::task::executor::deliver_webhook;
use bendclaw::kernel::task::executor::render_delivery_text;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;
use chrono::NaiveDateTime;
use chrono::Utc;
use tokio_util::sync::CancellationToken;

use crate::common::fake_databend::FakeDatabend;

fn sample_task() -> TaskRecord {
    TaskRecord {
        id: "task-1".to_string(),
        node_id: "inst-1".to_string(),
        name: "nightly-report".to_string(),
        prompt: "run report".to_string(),
        enabled: true,
        status: "idle".to_string(),
        schedule: TaskSchedule::Every { seconds: 60 },
        delivery: TaskDelivery::None,
        user_id: String::new(),
        scope: "private".to_string(),
        created_by: String::new(),
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

fn fake_pool(rows: Vec<Vec<serde_json::Value>>) -> bendclaw::storage::Pool {
    let rows = Arc::new(Mutex::new(rows));
    FakeDatabend::new(move |_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
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
        serde_json::Value::String("private".to_string()),
        serde_json::Value::String("".to_string()),
        serde_json::Value::String("".to_string()),
        serde_json::Value::String("{}".to_string()),
        serde_json::Value::String(if enabled { "1" } else { "0" }.to_string()),
        serde_json::Value::String("".to_string()),
        serde_json::Value::String("".to_string()),
        serde_json::Value::String("".to_string()),
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

    let (status, error) = deliver_webhook(&client, &url, &task, "error", None, Some("boom")).await;

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

#[test]
fn classify_task_run_output_marks_completed_runs_ok() {
    let (status, output, error) = classify_task_run_output(FinishedRunOutput {
        text: "done".to_string(),
        stop_reason: Reason::EndTurn,
    });
    assert_eq!(status, "ok");
    assert_eq!(output.as_deref(), Some("done"));
    assert!(error.is_none());
}

#[test]
fn classify_task_run_output_marks_budget_runs_partial() {
    let (status, output, error) = classify_task_run_output(FinishedRunOutput {
        text: "partial summary".to_string(),
        stop_reason: Reason::MaxIterations,
    });
    assert_eq!(status, "partial");
    assert_eq!(output.as_deref(), Some("partial summary"));
    assert!(error
        .as_deref()
        .is_some_and(|value| value.contains("max_iterations")));
}

#[test]
fn classify_task_run_output_marks_aborted_runs_cancelled() {
    let (status, _output, error) = classify_task_run_output(FinishedRunOutput {
        text: "".to_string(),
        stop_reason: Reason::Aborted,
    });
    assert_eq!(status, "cancelled");
    assert!(error.is_none());
}

#[test]
fn classify_task_run_output_marks_error_runs_error() {
    let (status, _output, error) = classify_task_run_output(FinishedRunOutput {
        text: "".to_string(),
        stop_reason: Reason::Error,
    });
    assert_eq!(status, "error");
    assert!(error.is_some());
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
