use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::channel::dispatcher::ChannelDispatcher;
use bendclaw::kernel::channel::InboundEvent;
use bendclaw::kernel::channel::InboundMessage;
use bendclaw::kernel::channel::ReplyContext;

// ── session_key ──

#[test]
fn session_key_format() {
    let key = ChannelDispatcher::session_key("telegram", "acc_1", "chat_42");
    assert_eq!(key, "telegram:acc_1:chat_42");
}

#[test]
fn session_key_http_api() {
    let key = ChannelDispatcher::session_key("http_api", "acc_x", "run_123");
    assert_eq!(key, "http_api:acc_x:run_123");
}

// ── extract_input: Message ──

#[test]
fn extract_input_message() -> Result<()> {
    let event = InboundEvent::Message(InboundMessage {
        message_id: "msg_1".into(),
        chat_id: "chat_1".into(),
        sender_id: "user_1".into(),
        sender_name: "Alice".into(),
        text: "hello agent".into(),
        attachments: vec![],
        timestamp: 1700000000,
    });
    let (text, reply_ctx) = ChannelDispatcher::extract_input(&event);
    assert_eq!(text, "hello agent");
    let ctx = reply_ctx.context("expected reply_ctx")?;
    assert_eq!(ctx.chat_id, "chat_1");
    assert_eq!(ctx.reply_to_message_id.as_deref(), Some("msg_1"));
    assert!(ctx.thread_id.is_none());
    Ok(())
}

// ── extract_input: PlatformEvent ──

#[test]
fn extract_input_platform_event_string_payload() {
    let event = InboundEvent::PlatformEvent {
        event_type: "push".into(),
        payload: serde_json::json!("refs/heads/main"),
        reply_context: None,
    };
    let (text, reply_ctx) = ChannelDispatcher::extract_input(&event);
    assert_eq!(text, "[push] refs/heads/main");
    assert!(reply_ctx.is_none());
}

#[test]
fn extract_input_platform_event_object_payload() -> Result<()> {
    let event = InboundEvent::PlatformEvent {
        event_type: "pull_request.opened".into(),
        payload: serde_json::json!({"number": 42, "title": "fix bug"}),
        reply_context: Some(ReplyContext {
            chat_id: "pr_42".into(),
            reply_to_message_id: None,
            thread_id: Some("42".into()),
        }),
    };
    let (text, reply_ctx) = ChannelDispatcher::extract_input(&event);
    assert!(text.starts_with("[pull_request.opened] "));
    assert!(text.contains("42"));
    let ctx = reply_ctx.context("expected reply_ctx")?;
    assert_eq!(ctx.chat_id, "pr_42");
    assert_eq!(ctx.thread_id.as_deref(), Some("42"));
    Ok(())
}

// ── extract_input: Callback ──

#[test]
fn extract_input_callback() -> Result<()> {
    let event = InboundEvent::Callback {
        callback_id: "cb_1".into(),
        data: "approve".into(),
        reply_context: Some(ReplyContext {
            chat_id: "chat_5".into(),
            reply_to_message_id: Some("msg_10".into()),
            thread_id: None,
        }),
    };
    let (text, reply_ctx) = ChannelDispatcher::extract_input(&event);
    assert_eq!(text, "approve");
    let ctx = reply_ctx.context("expected reply_ctx")?;
    assert_eq!(ctx.chat_id, "chat_5");
    assert_eq!(ctx.reply_to_message_id.as_deref(), Some("msg_10"));
    Ok(())
}

#[test]
fn extract_input_callback_no_reply_context() {
    let event = InboundEvent::Callback {
        callback_id: "cb_2".into(),
        data: "reject".into(),
        reply_context: None,
    };
    let (text, reply_ctx) = ChannelDispatcher::extract_input(&event);
    assert_eq!(text, "reject");
    assert!(reply_ctx.is_none());
}
