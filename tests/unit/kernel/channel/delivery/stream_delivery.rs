use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::channels::egress::stream_delivery::StreamDelivery;
use bendclaw::kernel::channels::egress::stream_delivery::StreamDeliveryConfig;
use bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound;
use bendclaw::kernel::run::event::Delta;
use bendclaw::kernel::run::event::Event;
use bendclaw::kernel::tools::OpType;
use bendclaw::kernel::OperationMeta;
use parking_lot::Mutex;

#[derive(Debug, Clone)]
#[allow(clippy::enum_variant_names)]
enum OutboundCall {
    SendDraft { text: String },
    UpdateDraft { msg_id: String, text: String },
    FinalizeDraft { msg_id: String, text: String },
}

struct MockOutbound {
    calls: Arc<Mutex<Vec<OutboundCall>>>,
    draft_msg_id: String,
}

impl MockOutbound {
    fn new(draft_msg_id: &str) -> (Self, Arc<Mutex<Vec<OutboundCall>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ob = Self {
            calls: calls.clone(),
            draft_msg_id: draft_msg_id.to_string(),
        };
        (ob, calls)
    }
}

#[async_trait]
impl ChannelOutbound for MockOutbound {
    async fn send_text(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
    ) -> bendclaw::types::Result<String> {
        Ok(self.draft_msg_id.clone())
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn edit_message(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn add_reaction(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::types::Result<()> {
        Ok(())
    }
    async fn send_draft(
        &self,
        _: &serde_json::Value,
        _: &str,
        text: &str,
    ) -> bendclaw::types::Result<String> {
        self.calls.lock().push(OutboundCall::SendDraft {
            text: text.to_string(),
        });
        Ok(self.draft_msg_id.clone())
    }
    async fn update_draft(
        &self,
        _: &serde_json::Value,
        _: &str,
        msg_id: &str,
        text: &str,
    ) -> bendclaw::types::Result<()> {
        self.calls.lock().push(OutboundCall::UpdateDraft {
            msg_id: msg_id.to_string(),
            text: text.to_string(),
        });
        Ok(())
    }
    async fn finalize_draft(
        &self,
        _: &serde_json::Value,
        _: &str,
        msg_id: &str,
        text: &str,
    ) -> bendclaw::types::Result<()> {
        self.calls.lock().push(OutboundCall::FinalizeDraft {
            msg_id: msg_id.to_string(),
            text: text.to_string(),
        });
        Ok(())
    }
}

fn default_config() -> StreamDeliveryConfig {
    StreamDeliveryConfig {
        throttle_ms: 0,
        min_initial_chars: 5,
        max_message_len: 4096,
        show_tool_progress: true,
    }
}

fn text_delta(s: &str) -> Event {
    Event::StreamDelta(Delta::Text {
        content: s.to_string(),
    })
}

fn tool_start(name: &str) -> Event {
    Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: name.into(),
        arguments: serde_json::Value::Null,
    }
}

fn tool_end(name: &str, success: bool) -> Event {
    Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: name.into(),
        success,
        output: String::new(),
        operation: OperationMeta {
            op_type: OpType::Execute,
            impact: None,
            timeout_secs: None,
            duration_ms: 0,
            summary: String::new(),
        },
    }
}

fn make_stream(events: Vec<Event>) -> tokio_stream::wrappers::ReceiverStream<Event> {
    let (tx, rx) = tokio::sync::mpsc::channel(events.len() + 1);
    for ev in events {
        tx.try_send(ev).ok();
    }
    drop(tx);
    tokio_stream::wrappers::ReceiverStream::new(rx)
}

#[tokio::test]
async fn sends_draft_after_min_chars_then_finalizes() {
    let (ob, calls) = MockOutbound::new("msg_42");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![text_delta("Hello"), text_delta(" world")]);
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result, "Hello world");

    let calls = calls.lock();
    assert!(matches!(&calls[0], OutboundCall::SendDraft { .. }));
    assert!(matches!(&calls[1], OutboundCall::UpdateDraft { msg_id, .. } if msg_id == "msg_42"));
    let last = calls.last().unwrap();
    assert!(
        matches!(last, OutboundCall::FinalizeDraft { msg_id, text } if msg_id == "msg_42" && text == "Hello world")
    );
}

#[tokio::test]
async fn no_draft_when_text_below_min_chars() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![text_delta("Hi")]);
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result, "Hi");
    assert!(calls.lock().is_empty());
}

#[tokio::test]
async fn empty_stream_returns_empty_string() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![]);
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert!(result.is_empty());
    assert!(calls.lock().is_empty());
}

#[tokio::test]
async fn tool_progress_shown_in_updates() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![
        text_delta("Hello world, this is a test"),
        tool_start("web_search"),
        tool_end("web_search", true),
    ]);
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result, "Hello world, this is a test");

    let calls = calls.lock();
    assert!(calls.len() >= 3);
    let tool_start_call = calls.iter().find(
        |c| matches!(c, OutboundCall::UpdateDraft { text, .. } if text.contains("web_search...")),
    );
    assert!(tool_start_call.is_some());
    let tool_end_call = calls
        .iter()
        .find(|c| matches!(c, OutboundCall::UpdateDraft { text, .. } if text.contains("\u{2705}")));
    assert!(tool_end_call.is_some());
}

#[tokio::test]
async fn reason_start_clears_tool_status() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![
        text_delta("Hello world"),
        tool_start("search"),
        Event::ReasonStart,
        text_delta(" more text"),
    ]);
    let _ = delivery.deliver(&mut stream).await.unwrap();

    let calls = calls.lock();
    let last_update = calls
        .iter()
        .rev()
        .find(|c| matches!(c, OutboundCall::UpdateDraft { .. }));
    if let Some(OutboundCall::UpdateDraft { text, .. }) = last_update {
        assert!(!text.contains("search"));
    }
}

#[tokio::test]
async fn truncates_long_output_on_finalize() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let config = StreamDeliveryConfig {
        throttle_ms: 0,
        min_initial_chars: 5,
        max_message_len: 20,
        show_tool_progress: false,
    };
    let delivery =
        StreamDelivery::new(config, Arc::new(ob), serde_json::json!({}), "chat_1".into());
    let long_text = "a".repeat(50);
    let mut stream = make_stream(vec![text_delta(&long_text)]);
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result.len(), 50);

    let calls = calls.lock();
    let finalize = calls
        .iter()
        .find(|c| matches!(c, OutboundCall::FinalizeDraft { .. }));
    if let Some(OutboundCall::FinalizeDraft { text, .. }) = finalize {
        assert!(text.len() <= 20);
    } else {
        panic!("expected a FinalizeDraft call");
    }
}

#[tokio::test]
async fn failed_tool_shows_error_icon() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![
        text_delta("Processing request"),
        tool_start("db_query"),
        tool_end("db_query", false),
    ]);
    let _ = delivery.deliver(&mut stream).await.unwrap();

    let calls = calls.lock();
    let fail_call = calls
        .iter()
        .find(|c| matches!(c, OutboundCall::UpdateDraft { text, .. } if text.contains("\u{274C}")));
    assert!(fail_call.is_some());
}

#[tokio::test]
async fn cursor_indicator_in_draft_updates_but_not_in_finalize() {
    let (ob, calls) = MockOutbound::new("msg_1");
    let delivery = StreamDelivery::new(
        default_config(),
        Arc::new(ob),
        serde_json::json!({}),
        "chat_1".into(),
    );
    let mut stream = make_stream(vec![text_delta("Hello world"), text_delta(" again")]);
    let _ = delivery.deliver(&mut stream).await.unwrap();

    let calls = calls.lock();
    for call in calls.iter() {
        match call {
            OutboundCall::SendDraft { text } | OutboundCall::UpdateDraft { text, .. } => {
                assert!(text.contains('\u{2026}'));
            }
            OutboundCall::FinalizeDraft { text, .. } => {
                assert!(!text.contains('\u{2026}'));
            }
        }
    }
}
