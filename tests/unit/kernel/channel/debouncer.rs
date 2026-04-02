use std::time::Duration;

use bendclaw::kernel::channels::model::account::ChannelAccount;
use bendclaw::kernel::channels::model::message::InboundEvent;
use bendclaw::kernel::channels::model::message::InboundMessage;
use bendclaw::kernel::channels::routing::debouncer::debounce;
use bendclaw::kernel::channels::routing::debouncer::ChatJob;
use bendclaw::kernel::channels::routing::debouncer::DebounceConfig;
use bendclaw::kernel::channels::routing::debouncer::DebounceResult;
use tokio::sync::mpsc;

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

fn msg_event(sender_id: &str, text: &str) -> InboundEvent {
    InboundEvent::Message(InboundMessage {
        message_id: format!("msg_{text}"),
        chat_id: "chat_1".into(),
        sender_id: sender_id.into(),
        sender_name: "Test".into(),
        text: text.into(),
        attachments: vec![],
        timestamp: 0,
    })
}

fn job(sender_id: &str, text: &str) -> ChatJob {
    ChatJob {
        account: test_account(),
        event: msg_event(sender_id, text),
    }
}

#[tokio::test]
async fn merge_rapid_messages() {
    let (tx, mut rx) = mpsc::channel::<ChatJob>(32);
    let config = DebounceConfig {
        window: Duration::from_millis(100),
        max_wait: Duration::from_secs(2),
    };

    // Push 2 more messages before debounce window expires.
    tx.send(job("alice", "second")).await.ok();
    tx.send(job("alice", "third")).await.ok();

    let result = debounce(&config, job("alice", "first"), &mut rx).await;
    match result {
        DebounceResult::Ready(input) => {
            assert_eq!(input.merged_count, 3);
            assert!(input.text.contains("first"));
            assert!(input.text.contains("second"));
            assert!(input.text.contains("third"));
        }
        DebounceResult::ReadyWithLeftover(..) => panic!("expected Ready, got ReadyWithLeftover"),
    }
}

#[tokio::test]
async fn control_command_bypass() {
    let (_tx, mut rx) = mpsc::channel::<ChatJob>(32);
    let config = DebounceConfig {
        window: Duration::from_millis(100),
        max_wait: Duration::from_secs(2),
    };

    for cmd in ["/new", "/clear", "/cancel", "/status", "/stop", "/abort"] {
        let result = debounce(&config, job("alice", cmd), &mut rx).await;
        match result {
            DebounceResult::Ready(input) => {
                assert_eq!(
                    input.merged_count, 1,
                    "control command {cmd} should not merge"
                );
                assert_eq!(input.text, cmd);
            }
            DebounceResult::ReadyWithLeftover(..) => {
                panic!("control command {cmd} should return Ready")
            }
        }
    }
}

#[tokio::test]
async fn single_message_no_delay_beyond_window() {
    let (_tx, mut rx) = mpsc::channel::<ChatJob>(32);
    let config = DebounceConfig {
        window: Duration::from_millis(50),
        max_wait: Duration::from_secs(2),
    };

    let start = tokio::time::Instant::now();
    let result = debounce(&config, job("alice", "hello"), &mut rx).await;
    let elapsed = start.elapsed();

    match result {
        DebounceResult::Ready(input) => {
            assert_eq!(input.merged_count, 1);
            assert_eq!(input.text, "hello");
        }
        DebounceResult::ReadyWithLeftover(..) => panic!("expected Ready"),
    }
    // Should return after ~window, not max_wait.
    assert!(
        elapsed < Duration::from_millis(200),
        "took too long: {elapsed:?}"
    );
}

#[tokio::test]
async fn different_sender_no_merge() {
    let (tx, mut rx) = mpsc::channel::<ChatJob>(32);
    let config = DebounceConfig {
        window: Duration::from_millis(100),
        max_wait: Duration::from_secs(2),
    };

    tx.send(job("bob", "from bob")).await.ok();

    let result = debounce(&config, job("alice", "from alice"), &mut rx).await;
    match result {
        DebounceResult::ReadyWithLeftover(input, leftover) => {
            assert_eq!(input.merged_count, 1);
            assert_eq!(input.text, "from alice");
            // Leftover should be bob's message.
            if let InboundEvent::Message(msg) = &leftover.event {
                assert_eq!(msg.sender_id, "bob");
            } else {
                panic!("expected Message event in leftover");
            }
        }
        DebounceResult::Ready(_) => panic!("expected ReadyWithLeftover"),
    }
}

#[tokio::test]
async fn max_wait_cap() {
    let (tx, mut rx) = mpsc::channel::<ChatJob>(32);
    let config = DebounceConfig {
        window: Duration::from_millis(80),
        max_wait: Duration::from_millis(200),
    };

    // Spawn a task that keeps sending messages every 50ms.
    let sender = tokio::spawn(async move {
        for i in 0..20 {
            if tx.send(job("alice", &format!("msg_{i}"))).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let start = tokio::time::Instant::now();
    let result = debounce(&config, job("alice", "first"), &mut rx).await;
    let elapsed = start.elapsed();

    sender.abort();

    match result {
        DebounceResult::Ready(input) => {
            assert!(
                input.merged_count >= 2,
                "should merge at least 2, got {}",
                input.merged_count
            );
            // Should not exceed max_wait + some tolerance.
            assert!(
                elapsed < Duration::from_millis(400),
                "should respect max_wait, took {elapsed:?}"
            );
        }
        DebounceResult::ReadyWithLeftover(..) => panic!("expected Ready"),
    }
}
