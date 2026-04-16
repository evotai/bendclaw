use std::time::Duration;

use evot::gateway::channels::feishu::config::FeishuChannelConfig;
use evot::gateway::channels::feishu::message::parse_event;
use evot::gateway::channels::feishu::message::MessageDedup;
use evot::gateway::channels::feishu::message::MessagePart;

fn default_config() -> FeishuChannelConfig {
    FeishuChannelConfig {
        app_id: "app_id".into(),
        app_secret: "app_secret".into(),
        mention_only: false,
        allow_from: vec![],
    }
}

fn make_event(extra_message_fields: serde_json::Value) -> serde_json::Value {
    let mut message = serde_json::json!({
        "message_id": "om_test_001",
        "chat_id": "oc_chat_001",
        "chat_type": "p2p",
        "message_type": "text",
        "content": r#"{"text":"hello"}"#,
    });
    if let (Some(base), Some(extra)) = (message.as_object_mut(), extra_message_fields.as_object()) {
        for (k, v) in extra {
            base.insert(k.clone(), v.clone());
        }
    }
    serde_json::json!({
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_id": { "open_id": "ou_sender_001" } },
            "message": message,
        }
    })
}

// ── parent_id extraction ──

#[test]
fn test_parse_event_extracts_parent_id() {
    let event = make_event(serde_json::json!({ "parent_id": "om_parent_123" }));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_some());
    let msg = parsed.unwrap();
    assert_eq!(msg.parent_id, Some("om_parent_123".to_string()));
}

#[test]
fn test_parse_event_no_parent_id() {
    let event = make_event(serde_json::json!({}));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().parent_id, None);
}

#[test]
fn test_parse_event_empty_parent_id() {
    let event = make_event(serde_json::json!({ "parent_id": "" }));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_some());
    assert_eq!(parsed.unwrap().parent_id, None);
}

// ── image message type ──

#[test]
fn test_parse_event_image_message() {
    let event = make_event(serde_json::json!({
        "message_type": "image",
        "content": r#"{"image_key":"img_v2_abc123"}"#,
    }));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_some());
    let msg = parsed.unwrap();
    assert!(matches!(
        msg.parts.as_slice(),
        [MessagePart::ImageKey(key)] if key == "img_v2_abc123"
    ));
    assert!(msg.text.is_empty());
}

#[test]
fn test_parse_event_image_message_empty_key() {
    let event = make_event(serde_json::json!({
        "message_type": "image",
        "content": r#"{"image_key":""}"#,
    }));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_none());
}

// ── post with images ──

#[test]
fn test_parse_post_extracts_image_keys() {
    let content = serde_json::json!({
        "content": [
            [
                { "tag": "text", "text": "Check this image" },
                { "tag": "img", "image_key": "img_v2_key1" }
            ],
            [
                { "tag": "img", "image_key": "img_v2_key2" }
            ]
        ]
    });
    let result = evot::gateway::channels::feishu::message::parse_post(&content);
    assert!(result.is_some());
    let post = result.unwrap();
    assert!(post.text.contains("Check this image"));
    assert!(matches!(
        post.parts.as_slice(),
        [MessagePart::Text(_), MessagePart::ImageKey(k1), MessagePart::ImageKey(k2)]
        if k1 == "img_v2_key1" && k2 == "img_v2_key2"
    ));
}

#[test]
fn test_parse_post_image_only() {
    let content = serde_json::json!({
        "content": [
            [
                { "tag": "img", "image_key": "img_v2_only" }
            ]
        ]
    });
    let result = evot::gateway::channels::feishu::message::parse_post(&content);
    assert!(result.is_some());
    let post = result.unwrap();
    assert!(post.text.is_empty());
    assert!(matches!(
        post.parts.as_slice(),
        [MessagePart::ImageKey(key)] if key == "img_v2_only"
    ));
}

#[test]
fn test_parse_event_post_with_image() {
    let post_content = serde_json::json!({
        "content": [
            [
                { "tag": "text", "text": "Look at this" },
                { "tag": "img", "image_key": "img_v2_post" }
            ]
        ]
    });
    let event = make_event(serde_json::json!({
        "message_type": "post",
        "content": post_content.to_string(),
    }));
    let config = default_config();
    let mut dedup = MessageDedup::new(Duration::from_secs(60));

    let parsed = parse_event(&event, &config, "bot_id", &mut dedup);
    assert!(parsed.is_some());
    let msg = parsed.unwrap();
    assert!(msg.text.contains("Look at this"));
    assert!(matches!(
        msg.parts.as_slice(),
        [MessagePart::Text(text), MessagePart::ImageKey(key)]
        if text == "Look at this" && key == "img_v2_post"
    ));
}

#[test]
fn test_parse_post_preserves_text_image_order() {
    let content = serde_json::json!({
        "content": [
            [
                { "tag": "text", "text": "before" },
                { "tag": "img", "image_key": "img_v2_a" },
                { "tag": "text", "text": "after" }
            ]
        ]
    });
    let result = evot::gateway::channels::feishu::message::parse_post(&content);
    let post = result.expect("post");
    assert!(matches!(
        post.parts.as_slice(),
        [MessagePart::Text(a), MessagePart::ImageKey(k), MessagePart::Text(b)]
        if a == "before" && k == "img_v2_a" && b == "after"
    ));
}

#[test]
fn test_parse_post_extracts_text() {
    let content = serde_json::json!({
        "title": "Title",
        "content": [
            [
                { "tag": "text", "text": "Hello " },
                { "tag": "text", "text": "world" }
            ]
        ]
    });
    let result = evot::gateway::channels::feishu::message::parse_post(&content);
    assert!(result.is_some());
    let post = result.unwrap();
    assert!(post.text.contains("Hello"));
    assert!(post.text.contains("world"));
    assert!(post.text.contains("Title"));
}

#[test]
fn test_parse_post_empty_content() {
    let content = serde_json::json!({ "content": [] });
    let result = evot::gateway::channels::feishu::message::parse_post(&content);
    assert!(result.is_none());
}
