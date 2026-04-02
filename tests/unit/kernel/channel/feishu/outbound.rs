use bendclaw::kernel::channels::adapters::feishu::FeishuChannel;
use bendclaw::kernel::channels::ChannelPlugin;

#[tokio::test]
async fn outbound_send_text_extract_credentials_error() {
    let ch = FeishuChannel::new();
    let outbound = ch.outbound();
    let result = outbound
        .send_text(&serde_json::json!({}), "chat_1", "hello")
        .await;
    assert!(result.is_err());
}
