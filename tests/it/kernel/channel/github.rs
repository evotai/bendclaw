use bendclaw::kernel::channel::ChannelKind;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::InboundEvent;
use bendclaw::kernel::channel::InboundKind;
use bendclaw::kernel::channel::InboundMode;
use bendclaw::kernel::channel::WebhookHandler;
use bendclaw::kernel::channel::plugins::github::GitHubChannel;
use bendclaw::kernel::channel::plugins::github::GitHubWebhookHandler;
use bendclaw::kernel::channel::plugins::github::GITHUB_CHANNEL_TYPE;

#[test]
fn channel_type() {
    let ch = GitHubChannel::new();
    assert_eq!(ch.channel_type(), "github");
    assert_eq!(ch.channel_type(), GITHUB_CHANNEL_TYPE);
}

#[test]
fn default_constructor() {
    let ch = GitHubChannel::default();
    assert_eq!(ch.channel_type(), "github");
}

#[test]
fn capabilities() {
    let ch = GitHubChannel::new();
    let caps = ch.capabilities();
    assert_eq!(caps.channel_kind, ChannelKind::EventDriven);
    assert_eq!(caps.inbound_mode, InboundMode::Webhook);
    assert!(!caps.supports_edit);
    assert!(!caps.supports_streaming);
    assert!(caps.supports_markdown);
    assert!(caps.supports_threads);
    assert!(caps.supports_reactions);
    assert_eq!(caps.max_message_len, 65_536);
}

#[test]
fn inbound_is_webhook() {
    let ch = GitHubChannel::new();
    assert!(matches!(ch.inbound(), InboundKind::Webhook(_)));
}

#[tokio::test]
async fn outbound_send_typing_is_noop() {
    let ch = GitHubChannel::new();
    let outbound = ch.outbound();
    outbound.send_typing(&serde_json::json!({"token": "ghp_test"}), "chat").await.unwrap();
}

#[tokio::test]
async fn outbound_edit_message_returns_error() {
    let ch = GitHubChannel::new();
    let outbound = ch.outbound();
    let result = outbound.edit_message(&serde_json::json!({"token": "ghp_test"}), "c", "m", "text").await;
    assert!(result.is_err());
}

// ── Webhook parsing ──

fn handler() -> GitHubWebhookHandler {
    GitHubWebhookHandler
}

#[test]
fn verify_accepts_valid_json() {
    let h = handler();
    let headers = axum::http::HeaderMap::new();
    h.verify("acc", &headers, br#"{"action":"opened"}"#).unwrap();
}

#[test]
fn verify_rejects_invalid_json() {
    let h = handler();
    let headers = axum::http::HeaderMap::new();
    assert!(h.verify("acc", &headers, b"not json").is_err());
}

#[test]
fn parse_pull_request_opened() {
    let h = handler();
    let body = serde_json::to_vec(&serde_json::json!({
        "action": "opened",
        "pull_request": {
            "number": 42,
            "title": "Add feature X",
            "body": "This PR adds feature X",
            "user": { "login": "alice" }
        },
        "repository": { "full_name": "org/repo" },
        "sender": { "login": "alice" }
    }))
    .unwrap();
    let events = h.parse("acc", &body).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        InboundEvent::PlatformEvent {
            event_type,
            payload,
            reply_context,
        } => {
            assert_eq!(event_type, "pull_request.opened");
            assert_eq!(payload["number"], 42);
            assert_eq!(payload["title"], "Add feature X");
            assert_eq!(payload["author"], "alice");
            let ctx = reply_context.as_ref().unwrap();
            assert_eq!(ctx.chat_id, "repos/org/repo/issues/42");
            assert_eq!(ctx.thread_id.as_deref(), Some("42"));
            assert!(ctx.reply_to_message_id.is_none());
        }
        _ => panic!("expected PlatformEvent"),
    }
}

#[test]
fn parse_issue_opened() {
    let h = handler();
    let body = serde_json::to_vec(&serde_json::json!({
        "action": "opened",
        "issue": {
            "number": 10,
            "title": "Bug report",
            "body": "Something is broken",
            "user": { "login": "bob" }
        },
        "repository": { "full_name": "org/repo" },
        "sender": { "login": "bob" }
    }))
    .unwrap();
    let events = h.parse("acc", &body).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        InboundEvent::PlatformEvent {
            event_type,
            reply_context,
            ..
        } => {
            assert_eq!(event_type, "issues.opened");
            let ctx = reply_context.as_ref().unwrap();
            assert_eq!(ctx.chat_id, "repos/org/repo/issues/10");
            assert_eq!(ctx.thread_id.as_deref(), Some("10"));
        }
        _ => panic!("expected PlatformEvent"),
    }
}

#[test]
fn parse_issue_comment_created() {
    let h = handler();
    let body = serde_json::to_vec(&serde_json::json!({
        "action": "created",
        "issue": {
            "number": 10,
            "title": "Bug report",
            "body": "Something is broken",
            "user": { "login": "bob" }
        },
        "comment": {
            "id": 999,
            "body": "I can reproduce this",
            "user": { "login": "carol" }
        },
        "repository": { "full_name": "org/repo" },
        "sender": { "login": "carol" }
    }))
    .unwrap();
    let events = h.parse("acc", &body).unwrap();
    assert_eq!(events.len(), 1);
    match &events[0] {
        InboundEvent::PlatformEvent {
            event_type,
            payload,
            reply_context,
        } => {
            assert_eq!(event_type, "issue_comment.created");
            assert_eq!(payload["comment_id"], 999);
            assert_eq!(payload["author"], "carol");
            let ctx = reply_context.as_ref().unwrap();
            assert_eq!(ctx.chat_id, "repos/org/repo/issues/10");
            assert_eq!(ctx.reply_to_message_id.as_deref(), Some("999"));
            assert_eq!(ctx.thread_id.as_deref(), Some("10"));
        }
        _ => panic!("expected PlatformEvent"),
    }
}

#[test]
fn parse_unknown_event_ignored() {
    let h = handler();
    let body = serde_json::to_vec(&serde_json::json!({
        "action": "completed",
        "workflow_run": { "id": 123 },
        "repository": { "full_name": "org/repo" }
    }))
    .unwrap();
    let events = h.parse("acc", &body).unwrap();
    assert!(events.is_empty());
}

#[test]
fn parse_invalid_body() {
    let h = handler();
    assert!(h.parse("acc", b"not json").is_err());
}
