use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use bendclaw::channels::model::account::ChannelAccount;
use bendclaw::channels::model::capabilities::ChannelCapabilities;
use bendclaw::channels::model::capabilities::ChannelKind;
use bendclaw::channels::model::capabilities::InboundMode;
use bendclaw::channels::runtime::channel_trait::ChannelOutbound;
use bendclaw::channels::runtime::channel_trait::ChannelPlugin;
use bendclaw::channels::runtime::channel_trait::InboundEventSender;
use bendclaw::channels::runtime::channel_trait::InboundKind;
use bendclaw::channels::runtime::channel_trait::ReceiverFactory;
use bendclaw::channels::ChannelRegistry;
use bendclaw::kernel::task::delivery::channel_delivery::deliver_channel;
use bendclaw::kernel::task::delivery::channel_delivery::render_delivery_text;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;
use bendclaw::types::Result as BaseResult;
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
            stale_event_threshold: None,
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
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> BaseResult<()> {
        Ok(())
    }
    async fn edit_message(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> BaseResult<()> {
        Ok(())
    }
    async fn add_reaction(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
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

#[test]
fn render_delivery_text_includes_output_and_error() {
    let text = render_delivery_text(&sample_task(), "error", Some("partial"), Some("boom"));
    assert!(text.contains("nightly-report"));
    assert!(text.contains("partial"));
    assert!(text.contains("Error: boom"));
}

#[tokio::test]
async fn reports_success() {
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
