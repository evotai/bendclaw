use bendclaw::kernel::channel::plugins::telegram::TelegramChannel;
use bendclaw::kernel::channel::plugins::telegram::TELEGRAM_CHANNEL_TYPE;
use bendclaw::kernel::channel::ChannelKind;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::InboundKind;
use bendclaw::kernel::channel::InboundMode;

#[test]
fn channel_type() {
    let ch = TelegramChannel::new();
    assert_eq!(ch.channel_type(), "telegram");
    assert_eq!(ch.channel_type(), TELEGRAM_CHANNEL_TYPE);
}

#[test]
fn default_constructor() {
    let ch = TelegramChannel::default();
    assert_eq!(ch.channel_type(), "telegram");
}

#[test]
fn capabilities() {
    let ch = TelegramChannel::new();
    let caps = ch.capabilities();
    assert_eq!(caps.channel_kind, ChannelKind::Conversational);
    assert_eq!(caps.inbound_mode, InboundMode::Polling);
    assert!(caps.supports_edit);
    assert!(!caps.supports_streaming);
    assert!(caps.supports_markdown);
    assert!(!caps.supports_threads);
    assert!(!caps.supports_reactions);
    assert_eq!(caps.max_message_len, 4096);
}

#[test]
fn inbound_is_receiver() {
    let ch = TelegramChannel::new();
    assert!(matches!(ch.inbound(), InboundKind::Receiver(_)));
}

#[tokio::test]
async fn validate_config_rejects_empty_token() {
    let ch = TelegramChannel::new();
    assert!(ch.validate_config(&serde_json::json!({})).is_err());
    assert!(ch
        .validate_config(&serde_json::json!({"token": ""}))
        .is_err());
    assert!(ch
        .validate_config(&serde_json::json!({"token": "123:ABC"}))
        .is_ok());
}

#[tokio::test]
async fn outbound_send_typing_does_not_panic() {
    let ch = TelegramChannel::new();
    let outbound = ch.outbound();
    let _ = outbound
        .send_typing(&serde_json::json!({"token": "fake_token"}), "fake_chat")
        .await;
}

#[tokio::test]
async fn outbound_edit_message_does_not_panic() {
    let ch = TelegramChannel::new();
    let outbound = ch.outbound();
    let _ = outbound
        .edit_message(
            &serde_json::json!({"token": "fake_token"}),
            "chat_1",
            "msg_1",
            "new text",
        )
        .await;
}

#[tokio::test]
async fn outbound_add_reaction_returns_error() {
    let ch = TelegramChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .add_reaction(
            &serde_json::json!({"token": "fake_token"}),
            "chat_1",
            "msg_1",
            "thumbsup",
        )
        .await;
    assert!(result.is_err());
}
