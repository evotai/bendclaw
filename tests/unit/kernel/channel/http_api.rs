use bendclaw::kernel::channel::plugins::http_api::HttpApiChannel;
use bendclaw::kernel::channel::plugins::http_api::HTTP_API_CHANNEL_TYPE;
use bendclaw::kernel::channel::ChannelKind;
use bendclaw::kernel::channel::ChannelPlugin;
use bendclaw::kernel::channel::InboundKind;
use bendclaw::kernel::channel::InboundMode;

#[test]
fn channel_type() {
    let ch = HttpApiChannel::new();
    assert_eq!(ch.channel_type(), "http_api");
    assert_eq!(ch.channel_type(), HTTP_API_CHANNEL_TYPE);
}

#[test]
fn default_constructor() {
    let ch = HttpApiChannel;
    assert_eq!(ch.channel_type(), "http_api");
}

#[test]
fn capabilities_conversational() {
    let ch = HttpApiChannel::new();
    let caps = ch.capabilities();
    assert_eq!(caps.channel_kind, ChannelKind::Conversational);
    assert_eq!(caps.inbound_mode, InboundMode::HttpRequest);
    assert!(caps.supports_streaming);
    assert!(caps.supports_markdown);
    assert!(!caps.supports_edit);
    assert!(!caps.supports_threads);
    assert!(!caps.supports_reactions);
    assert_eq!(caps.max_message_len, 1_000_000);
}

#[test]
fn inbound_is_none() {
    let ch = HttpApiChannel::new();
    assert!(matches!(ch.inbound(), InboundKind::None));
}

#[tokio::test]
async fn outbound_send_text_returns_error() {
    let ch = HttpApiChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .send_text(&serde_json::json!({}), "chat_1", "hello")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn outbound_send_typing_is_ok() -> anyhow::Result<()> {
    let ch = HttpApiChannel::new();
    let outbound = ch.outbound();
    outbound
        .send_typing(&serde_json::json!({}), "chat_1")
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

#[tokio::test]
async fn outbound_edit_message_returns_error() {
    let ch = HttpApiChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .edit_message(&serde_json::json!({}), "chat_1", "msg_1", "new text")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn outbound_add_reaction_returns_error() {
    let ch = HttpApiChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .add_reaction(&serde_json::json!({}), "chat_1", "msg_1", "thumbsup")
        .await;
    assert!(result.is_err());
}
