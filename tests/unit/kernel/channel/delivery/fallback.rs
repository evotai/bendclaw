use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::execution::event::Delta;
use bendclaw::execution::event::Event;
use bendclaw::kernel::channels::egress::fallback::DeliveryMethod;
use bendclaw::kernel::channels::egress::fallback::FallbackDelivery;
use bendclaw::kernel::channels::egress::retry::RetryConfig;
use bendclaw::kernel::channels::egress::stream_delivery::StreamDeliveryConfig;
use bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound;
use bendclaw::types::ErrorCode;
use bendclaw::types::Result;
use parking_lot::Mutex;

// ── Mock outbound ────────────────────────────────────────────────────────────

struct MockOutbound {
    send_draft_fail: bool,
    send_text_fail: bool,
    finalize_draft_fail: bool,
    calls: Arc<Mutex<Vec<String>>>,
}

impl MockOutbound {
    fn new(send_draft_fail: bool, send_text_fail: bool) -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ob = Arc::new(Self {
            send_draft_fail,
            send_text_fail,
            finalize_draft_fail: false,
            calls: calls.clone(),
        });
        (ob, calls)
    }

    fn with_finalize_fail() -> (Arc<Self>, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ob = Arc::new(Self {
            send_draft_fail: false,
            send_text_fail: false,
            finalize_draft_fail: true,
            calls: calls.clone(),
        });
        (ob, calls)
    }
}

#[async_trait]
impl ChannelOutbound for MockOutbound {
    async fn send_text(&self, _: &serde_json::Value, _: &str, _: &str) -> Result<String> {
        self.calls.lock().push("send_text".into());
        if self.send_text_fail {
            Err(ErrorCode::channel_send("send_text failed"))
        } else {
            Ok("msg_text".into())
        }
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> Result<()> {
        Ok(())
    }
    async fn edit_message(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        self.calls.lock().push("edit_message".into());
        Ok(())
    }
    async fn add_reaction(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    async fn send_draft(&self, _: &serde_json::Value, _: &str, _: &str) -> Result<String> {
        self.calls.lock().push("send_draft".into());
        if self.send_draft_fail {
            Err(ErrorCode::channel_send("send_draft failed"))
        } else {
            Ok("msg_draft".into())
        }
    }
    async fn update_draft(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        self.calls.lock().push("update_draft".into());
        Ok(())
    }
    async fn finalize_draft(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        self.calls.lock().push("finalize_draft".into());
        if self.finalize_draft_fail {
            Err(ErrorCode::channel_send("finalize_draft failed"))
        } else {
            Ok(())
        }
    }
}

fn make_stream_config() -> StreamDeliveryConfig {
    StreamDeliveryConfig {
        throttle_ms: 50,
        min_initial_chars: 5,
        max_message_len: 10000,
        show_tool_progress: false,
    }
}

fn make_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 2,
        min_delay_ms: 10,
        max_delay_ms: 50,
    }
}

fn text_events(text: &str) -> Vec<Event> {
    vec![Event::StreamDelta(Delta::Text {
        content: text.to_string(),
    })]
}

#[tokio::test]
async fn streaming_success_no_fallback() {
    let (ob, calls) = MockOutbound::new(false, false);
    let delivery = FallbackDelivery::new(
        make_stream_config(),
        ob,
        serde_json::Value::Null,
        "chat1".into(),
        make_retry_config(),
    );
    let mut stream = tokio_stream::iter(text_events("Hello, world!"));
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result.text, "Hello, world!");
    assert!(matches!(result.method, DeliveryMethod::Streamed));
    let c = calls.lock();
    assert!(c.contains(&"send_draft".to_string()));
    assert!(c.contains(&"finalize_draft".to_string()));
    assert!(!c.contains(&"send_text".to_string()));
}

#[tokio::test]
async fn fallback_to_send_text_when_draft_fails() {
    let (ob, calls) = MockOutbound::new(true, false);
    let delivery = FallbackDelivery::new(
        make_stream_config(),
        ob,
        serde_json::Value::Null,
        "chat1".into(),
        make_retry_config(),
    );
    let mut stream = tokio_stream::iter(text_events("Hello, world!"));
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result.text, "Hello, world!");
    assert!(matches!(result.method, DeliveryMethod::FellBack));
    assert!(!result.platform_message_id.is_empty());
    let c = calls.lock();
    assert!(c.contains(&"send_text".to_string()));
}

#[tokio::test]
async fn empty_output_returns_no_output() {
    let (ob, _calls) = MockOutbound::new(false, false);
    let delivery = FallbackDelivery::new(
        make_stream_config(),
        ob,
        serde_json::Value::Null,
        "chat1".into(),
        make_retry_config(),
    );
    let mut stream = tokio_stream::iter(text_events("   "));
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert!(matches!(result.method, DeliveryMethod::NoOutput));
}

#[tokio::test]
async fn fallback_when_finalize_draft_fails() {
    let (ob, calls) = MockOutbound::with_finalize_fail();
    let delivery = FallbackDelivery::new(
        make_stream_config(),
        ob,
        serde_json::Value::Null,
        "chat1".into(),
        make_retry_config(),
    );
    let mut stream = tokio_stream::iter(text_events("Hello, world!"));
    let result = delivery.deliver(&mut stream).await.unwrap();
    assert_eq!(result.text, "Hello, world!");
    assert!(matches!(result.method, DeliveryMethod::FellBack));
    assert!(!result.platform_message_id.is_empty());
    let c = calls.lock();
    // send_draft succeeded, finalize_draft was attempted, then fell back to send_text.
    assert!(c.contains(&"send_draft".to_string()));
    assert!(c.contains(&"finalize_draft".to_string()));
    assert!(c.contains(&"send_text".to_string()));
}
