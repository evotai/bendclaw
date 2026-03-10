use bendclaw::kernel::channel::Direction;
use bendclaw::kernel::channel::InboundEvent;
use bendclaw::kernel::channel::InboundMessage;
use bendclaw::kernel::channel::ReplyContext;

// ── InboundEvent serde roundtrip ──

#[test]
fn inbound_message_serde_roundtrip() {
    let event = InboundEvent::Message(InboundMessage {
        message_id: "msg_1".into(),
        chat_id: "chat_1".into(),
        sender_id: "user_1".into(),
        sender_name: "Alice".into(),
        text: "hello agent".into(),
        attachments: vec![],
        timestamp: 1700000000,
    });
    let json = serde_json::to_string(&event).unwrap();
    let parsed: InboundEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        InboundEvent::Message(msg) => {
            assert_eq!(msg.message_id, "msg_1");
            assert_eq!(msg.text, "hello agent");
            assert_eq!(msg.timestamp, 1700000000);
        }
        _ => panic!("expected Message variant"),
    }
}

#[test]
fn platform_event_serde_roundtrip() {
    let event = InboundEvent::PlatformEvent {
        event_type: "pull_request.opened".into(),
        payload: serde_json::json!({"number": 42}),
        reply_context: Some(ReplyContext {
            chat_id: "pr_42".into(),
            reply_to_message_id: None,
            thread_id: Some("42".into()),
        }),
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: InboundEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        InboundEvent::PlatformEvent {
            event_type,
            payload,
            reply_context,
        } => {
            assert_eq!(event_type, "pull_request.opened");
            assert_eq!(payload["number"], 42);
            let ctx = reply_context.unwrap();
            assert_eq!(ctx.thread_id.as_deref(), Some("42"));
        }
        _ => panic!("expected PlatformEvent variant"),
    }
}

#[test]
fn callback_serde_roundtrip() {
    let event = InboundEvent::Callback {
        callback_id: "cb_1".into(),
        data: "approve".into(),
        reply_context: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let parsed: InboundEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        InboundEvent::Callback {
            callback_id,
            data,
            reply_context,
        } => {
            assert_eq!(callback_id, "cb_1");
            assert_eq!(data, "approve");
            assert!(reply_context.is_none());
        }
        _ => panic!("expected Callback variant"),
    }
}

#[test]
fn platform_event_without_reply_context() {
    let event = InboundEvent::PlatformEvent {
        event_type: "push".into(),
        payload: serde_json::json!("refs/heads/main"),
        reply_context: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    // reply_context should be omitted from JSON
    assert!(!json.contains("reply_context"));
    let parsed: InboundEvent = serde_json::from_str(&json).unwrap();
    match parsed {
        InboundEvent::PlatformEvent { reply_context, .. } => {
            assert!(reply_context.is_none());
        }
        _ => panic!("expected PlatformEvent"),
    }
}

// ── Direction ──

#[test]
fn direction_as_str() {
    assert_eq!(Direction::Inbound.as_str(), "inbound");
    assert_eq!(Direction::Outbound.as_str(), "outbound");
}

#[test]
fn direction_serde_roundtrip() {
    let json = serde_json::to_string(&Direction::Inbound).unwrap();
    let parsed: Direction = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, Direction::Inbound);

    let json = serde_json::to_string(&Direction::Outbound).unwrap();
    let parsed: Direction = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, Direction::Outbound);
}
