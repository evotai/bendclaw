use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::channels::Direction;
use bendclaw::kernel::channels::InboundEvent;
use bendclaw::kernel::channels::InboundMessage;
use bendclaw::kernel::channels::ReplyContext;

// ── InboundEvent serde roundtrip ──

#[test]
fn inbound_message_serde_roundtrip() -> Result<()> {
    let event = InboundEvent::Message(InboundMessage {
        message_id: "msg_1".into(),
        chat_id: "chat_1".into(),
        sender_id: "user_1".into(),
        sender_name: "Alice".into(),
        text: "hello agent".into(),
        attachments: vec![],
        timestamp: 1700000000,
    });
    let json = serde_json::to_string(&event)?;
    let parsed: InboundEvent = serde_json::from_str(&json)?;
    match parsed {
        InboundEvent::Message(msg) => {
            assert_eq!(msg.message_id, "msg_1");
            assert_eq!(msg.text, "hello agent");
            assert_eq!(msg.timestamp, 1700000000);
        }
        _ => bail!("expected Message variant"),
    }
    Ok(())
}

#[test]
fn platform_event_serde_roundtrip() -> Result<()> {
    let event = InboundEvent::PlatformEvent {
        event_type: "pull_request.opened".into(),
        payload: serde_json::json!({"number": 42}),
        reply_context: Some(ReplyContext {
            chat_id: "pr_42".into(),
            reply_to_message_id: None,
            thread_id: Some("42".into()),
        }),
    };
    let json = serde_json::to_string(&event)?;
    let parsed: InboundEvent = serde_json::from_str(&json)?;
    match parsed {
        InboundEvent::PlatformEvent {
            event_type,
            payload,
            reply_context,
        } => {
            assert_eq!(event_type, "pull_request.opened");
            assert_eq!(payload["number"], 42);
            let ctx = reply_context.context("expected reply_context")?;
            assert_eq!(ctx.thread_id.as_deref(), Some("42"));
        }
        _ => bail!("expected PlatformEvent variant"),
    }
    Ok(())
}

#[test]
fn callback_serde_roundtrip() -> Result<()> {
    let event = InboundEvent::Callback {
        callback_id: "cb_1".into(),
        data: "approve".into(),
        reply_context: None,
    };
    let json = serde_json::to_string(&event)?;
    let parsed: InboundEvent = serde_json::from_str(&json)?;
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
        _ => bail!("expected Callback variant"),
    }
    Ok(())
}

#[test]
fn platform_event_without_reply_context() -> Result<()> {
    let event = InboundEvent::PlatformEvent {
        event_type: "push".into(),
        payload: serde_json::json!("refs/heads/main"),
        reply_context: None,
    };
    let json = serde_json::to_string(&event)?;
    // reply_context should be omitted from JSON
    assert!(!json.contains("reply_context"));
    let parsed: InboundEvent = serde_json::from_str(&json)?;
    match parsed {
        InboundEvent::PlatformEvent { reply_context, .. } => {
            assert!(reply_context.is_none());
        }
        _ => bail!("expected PlatformEvent"),
    }
    Ok(())
}

// ── Direction ──

#[test]
fn direction_as_str() {
    assert_eq!(Direction::Inbound.as_str(), "inbound");
    assert_eq!(Direction::Outbound.as_str(), "outbound");
}

#[test]
fn direction_serde_roundtrip() -> Result<()> {
    let json = serde_json::to_string(&Direction::Inbound)?;
    let parsed: Direction = serde_json::from_str(&json)?;
    assert_eq!(parsed, Direction::Inbound);

    let json = serde_json::to_string(&Direction::Outbound)?;
    let parsed: Direction = serde_json::from_str(&json)?;
    assert_eq!(parsed, Direction::Outbound);
    Ok(())
}
