use bendclaw::tasks::delivery::delivery_context::ChannelDeliveryContext;
use bendclaw::tasks::execution::prompt_builder::apply_channel_context;

#[test]
fn with_context_appends_channel_send_instructions() {
    let ctx = ChannelDeliveryContext {
        channel_type: "feishu".to_string(),
        chat_id: "chat-42".to_string(),
    };
    let result = apply_channel_context("run report", Some(&ctx));
    assert!(result.starts_with("run report"));
    assert!(result.contains("channel_type=\"feishu\""));
    assert!(result.contains("chat_id=\"chat-42\""));
    assert!(result.contains("channel_send"));
}

#[test]
fn without_context_appends_fallback_text() {
    let result = apply_channel_context("run report", None);
    assert!(result.starts_with("run report"));
    assert!(result.contains("Automatic delivery is configured by the system"));
    assert!(result.contains("produce a final response"));
}

#[test]
fn original_prompt_preserved_in_both_paths() {
    let prompt = "Generate a weekly summary of all open issues.";
    let ctx = ChannelDeliveryContext {
        channel_type: "telegram".to_string(),
        chat_id: "tg-99".to_string(),
    };
    let with = apply_channel_context(prompt, Some(&ctx));
    let without = apply_channel_context(prompt, None);
    assert!(with.starts_with(prompt));
    assert!(without.starts_with(prompt));
}
