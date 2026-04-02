use std::time::Duration;

use bendclaw::kernel::channels::adapters::feishu::message::parse_event;
use bendclaw::kernel::channels::adapters::feishu::message::parse_post;
use bendclaw::kernel::channels::adapters::feishu::message::should_respond_in_group;
use bendclaw::kernel::channels::adapters::feishu::message::strip_at_placeholders;
use bendclaw::kernel::channels::adapters::feishu::message::MessageDedup;
use bendclaw::kernel::channels::adapters::feishu::FeishuConfig;

#[test]
fn strip_at_placeholders_basic() {
    assert_eq!(
        strip_at_placeholders("hello @_user_1 world"),
        "hello  world"
    );
}

#[test]
fn strip_at_placeholders_multiple() {
    assert_eq!(strip_at_placeholders("@_user_1 @_user_2 hi"), "hi");
}

#[test]
fn strip_at_placeholders_no_match() {
    assert_eq!(strip_at_placeholders("hello world"), "hello world");
}

#[test]
fn parse_post_simple() {
    let content = serde_json::json!({
        "title": "Title",
        "content": [[
            {"tag": "text", "text": "Hello "},
            {"tag": "a", "text": "link"},
        ]]
    });
    let parsed = parse_post(&content).unwrap();
    assert_eq!(parsed.text, "Title\nHello link");
    assert!(parsed.mentioned_open_ids.is_empty());
}

#[test]
fn parse_post_with_at() {
    let content = serde_json::json!({
        "title": "",
        "content": [[
            {"tag": "text", "text": "Hey "},
            {"tag": "at", "user_id": "ou_abc", "user_name": "Bot"},
        ]]
    });
    let parsed = parse_post(&content).unwrap();
    assert_eq!(parsed.text, "Hey @Bot");
    assert_eq!(parsed.mentioned_open_ids, vec!["ou_abc"]);
}

#[test]
fn parse_post_empty_content() {
    let content = serde_json::json!({"title": "", "content": [[]]});
    assert!(parse_post(&content).is_none());
}

#[test]
fn should_respond_in_group_mention_only_true_with_mention() {
    let mentions = vec![serde_json::json!({
        "key": "@_user_1",
        "id": {"open_id": "ou_bot"},
    })];
    assert!(should_respond_in_group(true, "ou_bot", &mentions, &[]));
}

#[test]
fn should_respond_in_group_mention_only_true_without_mention() {
    let mentions = vec![serde_json::json!({
        "key": "@_user_1",
        "id": {"open_id": "ou_other"},
    })];
    assert!(!should_respond_in_group(true, "ou_bot", &mentions, &[]));
}

#[test]
fn should_respond_in_group_mention_only_false() {
    assert!(should_respond_in_group(false, "ou_bot", &[], &[]));
}

#[test]
fn should_respond_in_group_post_mention() {
    let post_ids = vec!["ou_bot".to_string()];
    assert!(should_respond_in_group(true, "ou_bot", &[], &post_ids));
}

#[test]
fn dedup_first_is_new() {
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    assert!(dedup.check_and_insert("msg_1"));
}

#[test]
fn dedup_second_is_duplicate() {
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    assert!(dedup.check_and_insert("msg_1"));
    assert!(!dedup.check_and_insert("msg_1"));
}

#[test]
fn dedup_different_ids() {
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    assert!(dedup.check_and_insert("msg_1"));
    assert!(dedup.check_and_insert("msg_2"));
}

fn make_config() -> FeishuConfig {
    FeishuConfig {
        app_id: "app123".to_string(),
        app_secret: "secret".to_string(),
        allow_from: vec![],
        mention_only: true,
    }
}

fn make_text_event(msg_id: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "header": {"event_type": "im.message.receive_v1"},
        "event": {
            "sender": {"sender_id": {"open_id": "ou_sender"}},
            "message": {
                "message_id": msg_id,
                "chat_id": "oc_chat",
                "message_type": "text",
                "chat_type": "p2p",
                "content": serde_json::json!({"text": text}).to_string(),
                "create_time": "1700000000000",
            }
        }
    })
}

#[test]
fn parse_event_text_message() {
    let config = make_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    let event = make_text_event("msg_1", "hello");
    let result = parse_event(&event, &config, &mut dedup);
    assert!(result.is_some());
    if let Some(bendclaw::kernel::channels::InboundEvent::Message(m)) = result {
        assert_eq!(m.text, "hello");
        assert_eq!(m.chat_id, "oc_chat");
        assert_eq!(m.sender_id, "ou_sender");
    }
}

#[test]
fn parse_event_dedup_blocks_second() {
    let config = make_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    let event = make_text_event("msg_dup", "hello");
    assert!(parse_event(&event, &config, &mut dedup).is_some());
    assert!(parse_event(&event, &config, &mut dedup).is_none());
}

#[test]
fn parse_event_unsupported_type() {
    let config = make_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    let event = serde_json::json!({
        "header": {"event_type": "im.message.receive_v1"},
        "event": {
            "sender": {"sender_id": {"open_id": "ou_sender"}},
            "message": {
                "message_id": "msg_img",
                "chat_id": "oc_chat",
                "message_type": "image",
                "chat_type": "p2p",
                "content": "{}",
                "create_time": "1700000000000",
            }
        }
    });
    assert!(parse_event(&event, &config, &mut dedup).is_none());
}

#[test]
fn parse_event_ignored_event_type() {
    let config = make_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));
    let event = serde_json::json!({
        "header": {"event_type": "im.chat.disbanded_v1"},
        "event": {}
    });
    assert!(parse_event(&event, &config, &mut dedup).is_none());
}
