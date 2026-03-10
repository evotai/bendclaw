use bendclaw::kernel::channel::plugins::feishu::FeishuChannel;
use bendclaw::kernel::channel::plugins::feishu::FEISHU_CHANNEL_TYPE;
use bendclaw::kernel::channel::ChannelKind;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::InboundKind;
use bendclaw::kernel::channel::InboundMode;

#[test]
fn channel_type() {
    let ch = FeishuChannel::new();
    assert_eq!(ch.channel_type(), "feishu");
    assert_eq!(ch.channel_type(), FEISHU_CHANNEL_TYPE);
}

#[test]
fn default_constructor() {
    let ch = FeishuChannel::default();
    assert_eq!(ch.channel_type(), "feishu");
}

#[test]
fn capabilities() {
    let ch = FeishuChannel::new();
    let caps = ch.capabilities();
    assert_eq!(caps.channel_kind, ChannelKind::Conversational);
    assert_eq!(caps.inbound_mode, InboundMode::WebSocket);
    assert!(!caps.supports_edit);
    assert!(!caps.supports_streaming);
    assert!(caps.supports_markdown);
    assert!(!caps.supports_threads);
    assert!(!caps.supports_reactions);
    assert_eq!(caps.max_message_len, 30_000);
}

#[test]
fn inbound_is_receiver() {
    let ch = FeishuChannel::new();
    assert!(matches!(ch.inbound(), InboundKind::Receiver(_)));
}

#[tokio::test]
async fn validate_config_rejects_empty_fields() {
    let ch = FeishuChannel::new();
    assert!(ch.validate_config(&serde_json::json!({})).is_err());
    assert!(ch
        .validate_config(&serde_json::json!({"app_id": "", "app_secret": "s"}))
        .is_err());
    assert!(ch
        .validate_config(&serde_json::json!({"app_id": "id", "app_secret": ""}))
        .is_err());
    assert!(ch
        .validate_config(&serde_json::json!({"app_id": "id", "app_secret": "s"}))
        .is_ok());
}

#[tokio::test]
async fn outbound_add_reaction_returns_error() {
    let ch = FeishuChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .add_reaction(
            &serde_json::json!({"app_id": "id", "app_secret": "secret"}),
            "chat_1",
            "msg_1",
            "thumbsup",
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn outbound_edit_message_returns_error() {
    let ch = FeishuChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .edit_message(
            &serde_json::json!({"app_id": "id", "app_secret": "secret"}),
            "chat_1",
            "msg_1",
            "new text",
        )
        .await;
    assert!(result.is_err());
}
