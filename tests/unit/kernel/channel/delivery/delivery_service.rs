use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::channels::egress::delivery_service::ChannelDeliveryService;
use bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound;
use bendclaw::types::ErrorCode;
use bendclaw::types::Result as BaseResult;
use parking_lot::Mutex;

struct MockOutbound {
    calls: Arc<Mutex<Vec<String>>>,
    fail: bool,
}

impl MockOutbound {
    fn new(fail: bool) -> (Self, Arc<Mutex<Vec<String>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                calls: calls.clone(),
                fail,
            },
            calls,
        )
    }
}

#[async_trait]
impl ChannelOutbound for MockOutbound {
    async fn send_text(
        &self,
        _config: &serde_json::Value,
        _chat_id: &str,
        text: &str,
    ) -> BaseResult<String> {
        if self.fail {
            return Err(ErrorCode::channel_send("send failed"));
        }
        self.calls.lock().push(text.to_string());
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

#[tokio::test]
async fn deliver_text_sends_and_returns_message_id() {
    let (ob, calls) = MockOutbound::new(false);
    let ob: Arc<dyn bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound> =
        Arc::new(ob);
    let result =
        ChannelDeliveryService::deliver_text(&ob, &serde_json::json!({}), "chat-1", "hello").await;
    assert!(result.is_ok());
    assert_eq!(calls.lock().as_slice(), ["hello"]);
}

#[tokio::test]
async fn deliver_text_returns_error_on_failure() {
    let (ob, _calls) = MockOutbound::new(true);
    let ob: Arc<dyn bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound> =
        Arc::new(ob);
    let result =
        ChannelDeliveryService::deliver_text(&ob, &serde_json::json!({}), "chat-1", "hello").await;
    assert!(result.is_err());
}
