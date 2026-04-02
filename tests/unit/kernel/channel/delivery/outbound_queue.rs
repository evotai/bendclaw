use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use bendclaw::base::ErrorCode;
use bendclaw::base::Result;
use bendclaw::kernel::channels::egress::outbound_queue::spawn_outbound_queue;
use bendclaw::kernel::channels::egress::outbound_queue::OutboundQueueConfig;
use bendclaw::kernel::channels::egress::outbound_queue::QueuedMessage;
use bendclaw::kernel::channels::egress::retry::RetryConfig;
use bendclaw::kernel::channels::runtime::channel_trait::ChannelOutbound;
use parking_lot::Mutex;

struct CountingOutbound {
    count: Arc<Mutex<usize>>,
    fail_until: usize,
}

#[async_trait]
impl ChannelOutbound for CountingOutbound {
    async fn send_text(&self, _: &serde_json::Value, _: &str, _: &str) -> Result<String> {
        let mut c = self.count.lock();
        *c += 1;
        if *c <= self.fail_until {
            Err(ErrorCode::channel_send("fail"))
        } else {
            Ok("ok".into())
        }
    }
    async fn send_typing(&self, _: &serde_json::Value, _: &str) -> Result<()> {
        Ok(())
    }
    async fn edit_message(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
    async fn add_reaction(&self, _: &serde_json::Value, _: &str, _: &str, _: &str) -> Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn queue_delivers_message() {
    let count = Arc::new(Mutex::new(0usize));
    let ob: Arc<dyn ChannelOutbound> = Arc::new(CountingOutbound {
        count: count.clone(),
        fail_until: 0,
    });

    let queue = spawn_outbound_queue(OutboundQueueConfig {
        capacity: 16,
        max_attempts: 3,
        retry_config: RetryConfig {
            max_retries: 1,
            min_delay_ms: 10,
            max_delay_ms: 50,
        },
    });

    queue.enqueue(QueuedMessage {
        outbound: ob,
        config: serde_json::Value::Null,
        chat_id: "c1".into(),
        text: "hello".into(),
        attempt: 1,
        next_attempt_at: Instant::now(),
    });

    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(*count.lock() >= 1);
}

#[tokio::test]
async fn queue_retries_on_failure_then_succeeds() {
    // fail_until=3 means first 3 send_text calls fail, 4th succeeds.
    // send_with_retry with max_retries=1 means each queue attempt does 2 calls.
    // So attempt 1: 2 calls (both fail), re-enqueue.
    // Attempt 2: 2 calls (call 3 fails, call 4 succeeds).
    let count = Arc::new(Mutex::new(0usize));
    let ob: Arc<dyn ChannelOutbound> = Arc::new(CountingOutbound {
        count: count.clone(),
        fail_until: 3,
    });

    let queue = spawn_outbound_queue(OutboundQueueConfig {
        capacity: 16,
        max_attempts: 5,
        retry_config: RetryConfig {
            max_retries: 1,
            min_delay_ms: 10,
            max_delay_ms: 50,
        },
    });

    queue.enqueue(QueuedMessage {
        outbound: ob,
        config: serde_json::Value::Null,
        chat_id: "c1".into(),
        text: "hello".into(),
        attempt: 1,
        next_attempt_at: Instant::now(),
    });

    // Give enough time for re-enqueue + backoff + retry.
    tokio::time::sleep(std::time::Duration::from_secs(4)).await;
    assert_eq!(*count.lock(), 4);
}

#[tokio::test]
async fn queue_no_head_of_line_blocking() {
    // Two messages enqueued: first has a 1s delay, second is immediate.
    // Without concurrent dispatch, second would be blocked by first.
    let count = Arc::new(Mutex::new(0usize));
    let ob: Arc<dyn ChannelOutbound> = Arc::new(CountingOutbound {
        count: count.clone(),
        fail_until: 0,
    });

    let queue = spawn_outbound_queue(OutboundQueueConfig {
        capacity: 16,
        max_attempts: 3,
        retry_config: RetryConfig {
            max_retries: 0,
            min_delay_ms: 10,
            max_delay_ms: 50,
        },
    });

    // First message: delayed by 1 second.
    queue.enqueue(QueuedMessage {
        outbound: ob.clone(),
        config: serde_json::Value::Null,
        chat_id: "c1".into(),
        text: "delayed".into(),
        attempt: 1,
        next_attempt_at: Instant::now() + std::time::Duration::from_secs(1),
    });

    // Second message: immediate.
    queue.enqueue(QueuedMessage {
        outbound: ob,
        config: serde_json::Value::Null,
        chat_id: "c2".into(),
        text: "immediate".into(),
        attempt: 1,
        next_attempt_at: Instant::now(),
    });

    // After 200ms, the immediate message should have been delivered
    // even though the delayed one hasn't fired yet.
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        *count.lock() >= 1,
        "immediate message should have been delivered"
    );
}
