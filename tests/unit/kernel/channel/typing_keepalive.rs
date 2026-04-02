use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bendclaw::kernel::channels::routing::typing_keepalive::TypingKeepalive;
use bendclaw::kernel::channels::routing::typing_keepalive::TypingKeepaliveConfig;
use bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound;

struct TypingMock {
    typing_count: Arc<AtomicUsize>,
    fail: bool,
}

impl TypingMock {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        Self {
            typing_count: counter,
            fail: false,
        }
    }

    fn failing(counter: Arc<AtomicUsize>) -> Self {
        Self {
            typing_count: counter,
            fail: true,
        }
    }
}

#[async_trait]
impl ChannelOutbound for TypingMock {
    async fn send_text(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<String> {
        Ok(String::new())
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> bendclaw::base::Result<()> {
        self.typing_count.fetch_add(1, Ordering::Relaxed);
        if self.fail {
            Err(bendclaw::base::ErrorCode::internal("mock typing error"))
        } else {
            Ok(())
        }
    }
    async fn edit_message(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn add_reaction(
        &self,
        _: &serde_json::Value,
        _: &str,
        _: &str,
        _: &str,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn periodic_refresh() {
    let count = Arc::new(AtomicUsize::new(0));
    let outbound: Arc<dyn ChannelOutbound> = Arc::new(TypingMock::new(count.clone()));

    let keepalive = TypingKeepalive::start(
        outbound,
        serde_json::json!({}),
        "chat_1".into(),
        TypingKeepaliveConfig {
            interval: Duration::from_millis(50),
            ttl: Duration::from_secs(10),
        },
    );

    tokio::time::sleep(Duration::from_millis(230)).await;
    let ticks = count.load(Ordering::Relaxed);
    assert!((3..=6).contains(&ticks), "expected ~4 ticks, got {ticks}");

    keepalive.stop();
}

#[tokio::test]
async fn ttl_expiry() {
    let count = Arc::new(AtomicUsize::new(0));
    let outbound: Arc<dyn ChannelOutbound> = Arc::new(TypingMock::new(count.clone()));

    let _keepalive = TypingKeepalive::start(
        outbound,
        serde_json::json!({}),
        "chat_2".into(),
        TypingKeepaliveConfig {
            interval: Duration::from_millis(50),
            ttl: Duration::from_millis(120),
        },
    );

    // Wait well past TTL.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let ticks = count.load(Ordering::Relaxed);
    // TTL=120ms, interval=50ms → ticks at 50ms, 100ms. At 150ms deadline exceeded → stop.
    assert!(ticks <= 3, "expected <=3 ticks before TTL, got {ticks}");
}

#[tokio::test]
async fn stop_cancels() {
    let count = Arc::new(AtomicUsize::new(0));
    let outbound: Arc<dyn ChannelOutbound> = Arc::new(TypingMock::new(count.clone()));

    let keepalive = TypingKeepalive::start(
        outbound,
        serde_json::json!({}),
        "chat_3".into(),
        TypingKeepaliveConfig {
            interval: Duration::from_millis(50),
            ttl: Duration::from_secs(10),
        },
    );

    tokio::time::sleep(Duration::from_millis(70)).await;
    keepalive.stop();

    let count_at_stop = count.load(Ordering::Relaxed);

    tokio::time::sleep(Duration::from_millis(200)).await;
    let count_after = count.load(Ordering::Relaxed);
    assert_eq!(count_at_stop, count_after, "should not tick after stop");
}

#[tokio::test]
async fn error_resilient() {
    let count = Arc::new(AtomicUsize::new(0));
    let outbound: Arc<dyn ChannelOutbound> = Arc::new(TypingMock::failing(count.clone()));

    let keepalive = TypingKeepalive::start(
        outbound,
        serde_json::json!({}),
        "chat_4".into(),
        TypingKeepaliveConfig {
            interval: Duration::from_millis(50),
            ttl: Duration::from_secs(10),
        },
    );

    tokio::time::sleep(Duration::from_millis(230)).await;
    let ticks = count.load(Ordering::Relaxed);
    assert!(
        ticks >= 3,
        "should keep ticking despite errors, got {ticks}"
    );

    keepalive.stop();
}
