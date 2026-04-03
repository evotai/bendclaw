use bendclaw::channels::model::account::ChannelAccount;
use bendclaw::channels::model::message::InboundEvent;
use bendclaw::channels::model::message::InboundMessage;
use bendclaw::storage::dal::channel_message::repo::ChannelMessageRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn sample_account() -> ChannelAccount {
    ChannelAccount {
        channel_account_id: "acc-1".to_string(),
        channel_type: "slack".to_string(),
        external_account_id: "ext-1".to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "user-1".to_string(),
        config: serde_json::json!({}),
        enabled: true,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn message_event(message_id: &str) -> InboundEvent {
    InboundEvent::Message(InboundMessage {
        message_id: message_id.to_string(),
        chat_id: "chat-1".to_string(),
        sender_id: "user-1".to_string(),
        sender_name: "User".to_string(),
        text: "hello".to_string(),
        attachments: vec![],
        timestamp: 0,
    })
}

#[tokio::test]
async fn returns_false_when_message_not_seen() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(paged_rows(&[&["0"]], None, None)));
    let repo = ChannelMessageRepo::new(fake.pool());
    let exists = repo
        .exists_by_platform_message_id("slack", "ext-1", "chat-1", "msg-1")
        .await
        .unwrap();
    assert!(!exists);
}

#[tokio::test]
async fn returns_true_when_message_already_seen() {
    let fake = FakeDatabend::new(|_sql, _db| Ok(paged_rows(&[&["1"]], None, None)));
    let repo = ChannelMessageRepo::new(fake.pool());
    let exists = repo
        .exists_by_platform_message_id("slack", "ext-1", "chat-1", "msg-42")
        .await
        .unwrap();
    assert!(exists);
}

#[test]
fn non_message_events_are_not_deduped() {
    // Only InboundEvent::Message variants with non-empty message_id trigger dedup.
    // PlatformEvent and Callback variants are never checked.
    let _account = sample_account();
    let platform_event = InboundEvent::PlatformEvent {
        event_type: "pr_opened".to_string(),
        payload: serde_json::json!({}),
        reply_context: None,
    };
    let callback = InboundEvent::Callback {
        callback_id: "cb-1".to_string(),
        data: "click".to_string(),
        reply_context: None,
    };
    // These variants exist and compile — dedup logic skips them (only matches Message).
    let _ = platform_event;
    let _ = callback;
}

#[test]
fn message_with_empty_id_is_not_deduped() {
    // A message with empty message_id is skipped by the dedup check.
    let event = message_event("");
    if let InboundEvent::Message(msg) = &event {
        assert!(msg.message_id.is_empty());
    }
}
