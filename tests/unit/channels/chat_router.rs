use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use bendclaw::channels::model::account::ChannelAccount;
use bendclaw::channels::model::message::InboundEvent;
use bendclaw::channels::model::message::InboundMessage;
use bendclaw::channels::routing::chat_router::ChatHandler;
use bendclaw::channels::routing::chat_router::ChatRouter;
use bendclaw::channels::routing::chat_router::ChatRouterConfig;
use bendclaw::channels::routing::debouncer::DebounceConfig;
use parking_lot::Mutex;

fn test_account() -> ChannelAccount {
    ChannelAccount {
        channel_account_id: "acc_1".into(),
        channel_type: "telegram".into(),
        external_account_id: "ext_1".into(),
        agent_id: "agent_1".into(),
        user_id: "user_1".into(),
        config: serde_json::json!({}),
        enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn msg_event(chat_id: &str, sender_id: &str, text: &str) -> InboundEvent {
    InboundEvent::Message(InboundMessage {
        message_id: format!("msg_{text}"),
        chat_id: chat_id.into(),
        sender_id: sender_id.into(),
        sender_name: "Test".into(),
        text: text.into(),
        attachments: vec![],
        timestamp: 0,
    })
}

/// Record (chat_id_from_event, text, timestamp_ms) for each handler call.
type CallLog = Arc<Mutex<Vec<(String, String, u64)>>>;

fn recording_handler(log: CallLog, delay: Duration) -> ChatHandler {
    Arc::new(move |input| {
        let log = log.clone();
        Box::pin(async move {
            let chat_id = match &input.primary_event {
                InboundEvent::Message(msg) => msg.chat_id.clone(),
                _ => "unknown".into(),
            };
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            log.lock().push((chat_id, input.text.clone(), ts));
            if !delay.is_zero() {
                tokio::time::sleep(delay).await;
            }
        })
    })
}

fn fast_debounce() -> DebounceConfig {
    DebounceConfig {
        window: Duration::from_millis(20),
        max_wait: Duration::from_millis(100),
    }
}

#[tokio::test]
async fn same_chat_serial() {
    let log: CallLog = Arc::new(Mutex::new(Vec::new()));
    // Each handler call takes 50ms — if serial, total ~250ms for 5 messages.
    let handler = recording_handler(log.clone(), Duration::from_millis(50));
    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig {
            per_chat_capacity: 32,
            idle_timeout: Duration::from_secs(5),
        },
        fast_debounce(),
        handler,
    ));

    for i in 0..5 {
        // Small delay between sends so debouncer doesn't merge them.
        tokio::time::sleep(Duration::from_millis(30)).await;
        router
            .route(
                test_account(),
                msg_event("chat_A", "alice", &format!("msg_{i}")),
            )
            .await;
    }

    // Wait for all to complete.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let calls = log.lock().clone();
    assert!(!calls.is_empty(), "should have processed messages");

    // Verify timestamps are monotonically increasing (serial execution).
    for window in calls.windows(2) {
        assert!(
            window[1].2 >= window[0].2,
            "calls should be serial: {:?} before {:?}",
            window[0],
            window[1]
        );
    }
}

#[tokio::test]
async fn different_chats_concurrent() {
    let counter = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let c = counter.clone();
    let mc = max_concurrent.clone();
    let handler: ChatHandler = Arc::new(move |_input| {
        let c = c.clone();
        let mc = mc.clone();
        Box::pin(async move {
            let current = c.fetch_add(1, Ordering::SeqCst) + 1;
            // Track max concurrency.
            mc.fetch_max(current, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(100)).await;
            c.fetch_sub(1, Ordering::SeqCst);
        })
    });

    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig {
            per_chat_capacity: 32,
            idle_timeout: Duration::from_secs(5),
        },
        fast_debounce(),
        handler,
    ));

    // Send to different chats — use join to enqueue concurrently.
    tokio::join!(
        router.route(test_account(), msg_event("chat_A", "alice", "hello_A")),
        router.route(test_account(), msg_event("chat_B", "bob", "hello_B")),
        router.route(test_account(), msg_event("chat_C", "carol", "hello_C")),
    );

    tokio::time::sleep(Duration::from_millis(300)).await;

    let peak = max_concurrent.load(Ordering::SeqCst);
    assert!(
        peak >= 2,
        "different chats should run concurrently, peak={peak}"
    );
}

#[tokio::test]
async fn idle_cleanup() {
    let log: CallLog = Arc::new(Mutex::new(Vec::new()));
    let handler = recording_handler(log.clone(), Duration::ZERO);
    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig {
            per_chat_capacity: 32,
            idle_timeout: Duration::from_millis(100),
        },
        fast_debounce(),
        handler,
    ));

    router
        .route(test_account(), msg_event("chat_A", "alice", "hello"))
        .await;

    // Wait for processing + debounce window.
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(router.active_chats().await, 1, "should have 1 active chat");

    // Wait for idle timeout.
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        router.active_chats().await,
        0,
        "should cleanup after idle timeout"
    );
}

#[tokio::test]
async fn no_chat_id_bypass() {
    let log: CallLog = Arc::new(Mutex::new(Vec::new()));
    let handler = recording_handler(log.clone(), Duration::ZERO);
    let router = Arc::new(ChatRouter::new(
        ChatRouterConfig::default(),
        fast_debounce(),
        handler,
    ));

    // PlatformEvent with no reply_context → no chat_id.
    let event = InboundEvent::PlatformEvent {
        event_type: "test".into(),
        payload: serde_json::json!({"key": "value"}),
        reply_context: None,
    };
    router.route(test_account(), event).await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    let calls = log.lock().clone();
    assert_eq!(calls.len(), 1, "should handle event without chat_id");
    assert_eq!(
        router.active_chats().await,
        0,
        "should not create chat queue"
    );
}
