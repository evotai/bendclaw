//! Feishu message parsing: text, post rich-text, @mention cleanup, dedup.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::kernel::channel::message::{InboundEvent, InboundMessage};

// ── Text cleanup ──────────────────────────────────────────────────────────────

/// Strip `@_user_N` placeholder tokens injected by Feishu in group chats.
pub fn strip_at_placeholders(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == '@' {
            let rest: String = chars.clone().map(|(_, c)| c).collect();
            if let Some(after) = rest.strip_prefix("_user_") {
                let skip =
                    "_user_".len() + after.chars().take_while(|c| c.is_ascii_digit()).count();
                for _ in 0..skip {
                    chars.next();
                }
                if chars.peek().map(|(_, c)| *c == ' ').unwrap_or(false) {
                    chars.next();
                }
                continue;
            }
        }
        result.push(ch);
    }
    result
}

// ── Post rich-text ────────────────────────────────────────────────────────────

pub struct ParsedPost {
    pub text: String,
    pub mentioned_open_ids: Vec<String>,
}

/// Flatten a Feishu `post` rich-text message to plain text.
pub fn parse_post(content: &str) -> Option<ParsedPost> {
    let parsed = serde_json::from_str::<serde_json::Value>(content).ok()?;
    let locale = parsed
        .get("zh_cn")
        .or_else(|| parsed.get("en_us"))
        .or_else(|| parsed.as_object().and_then(|m| m.values().find(|v| v.is_object())))?;

    let mut text = String::new();
    let mut mentioned_open_ids = Vec::new();

    if let Some(title) = locale.get("title").and_then(|t| t.as_str()).filter(|s| !s.is_empty()) {
        text.push_str(title);
        text.push_str("\n\n");
    }

    if let Some(paragraphs) = locale.get("content").and_then(|c| c.as_array()) {
        for para in paragraphs {
            if let Some(elements) = para.as_array() {
                for el in elements {
                    match el.get("tag").and_then(|t| t.as_str()).unwrap_or("") {
                        "text" => {
                            if let Some(t) = el.get("text").and_then(|t| t.as_str()) {
                                text.push_str(t);
                            }
                        }
                        "a" => {
                            text.push_str(
                                el.get("text")
                                    .and_then(|t| t.as_str())
                                    .filter(|s| !s.is_empty())
                                    .or_else(|| el.get("href").and_then(|h| h.as_str()))
                                    .unwrap_or(""),
                            );
                        }
                        "at" => {
                            let name = el
                                .get("user_name")
                                .and_then(|n| n.as_str())
                                .or_else(|| el.get("user_id").and_then(|i| i.as_str()))
                                .unwrap_or("user");
                            text.push('@');
                            text.push_str(name);
                            if let Some(open_id) = el
                                .get("user_id")
                                .and_then(|i| i.as_str())
                                .map(str::trim)
                                .filter(|id| !id.is_empty())
                            {
                                mentioned_open_ids.push(open_id.to_string());
                            }
                        }
                        _ => {}
                    }
                }
                text.push('\n');
            }
        }
    }

    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(ParsedPost { text, mentioned_open_ids })
}

// ── Group mention check ───────────────────────────────────────────────────────

/// In group chats, only respond when the bot is @-mentioned.
/// Returns true if mention_only is false, or if bot_open_id appears in mentions.
pub fn should_respond_in_group(
    mention_only: bool,
    bot_open_id: Option<&str>,
    mentions: &[serde_json::Value],
    post_mentioned_ids: &[String],
) -> bool {
    if !mention_only {
        return true;
    }
    let Some(bot_id) = bot_open_id.filter(|id| !id.is_empty()) else {
        return false;
    };
    mentions.iter().any(|m| {
        m.pointer("/id/open_id")
            .or_else(|| m.pointer("/open_id"))
            .and_then(|v| v.as_str())
            .is_some_and(|id| id == bot_id)
    }) || post_mentioned_ids.iter().any(|id| id == bot_id)
}

// ── Message dedup ─────────────────────────────────────────────────────────────

const DEDUP_WINDOW: Duration = Duration::from_secs(30 * 60);

pub struct MessageDedup {
    seen: HashMap<String, Instant>,
}

impl MessageDedup {
    pub fn new() -> Self {
        Self { seen: HashMap::new() }
    }

    /// Returns true if this message_id is new (not a duplicate).
    pub fn check_and_insert(&mut self, message_id: &str) -> bool {
        let now = Instant::now();
        self.seen.retain(|_, t| now.duration_since(*t) < DEDUP_WINDOW);
        if self.seen.contains_key(message_id) {
            return false;
        }
        self.seen.insert(message_id.to_string(), now);
        true
    }
}

impl Default for MessageDedup {
    fn default() -> Self {
        Self::new()
    }
}

// ── Event parsing ─────────────────────────────────────────────────────────────

/// Parse a Feishu `im.message.receive_v1` event into an InboundEvent.
/// Returns None for unsupported message types or empty text.
pub fn parse_event(
    event: &serde_json::Value,
    mention_only: bool,
    bot_open_id: Option<&str>,
) -> Option<InboundEvent> {
    let message = event.get("message")?;
    let sender_id = event
        .pointer("/sender/sender_id/open_id")
        .and_then(|v| v.as_str())?;

    let msg_id = message.get("message_id")?.as_str()?;
    let chat_id = message.get("chat_id")?.as_str()?;
    let chat_type = message.get("chat_type").and_then(|v| v.as_str()).unwrap_or("p2p");
    let msg_type = message.get("message_type")?.as_str()?;
    let content_str = message.get("content")?.as_str()?;
    let mentions: Vec<serde_json::Value> = message
        .get("mentions")
        .and_then(|m| m.as_array())
        .cloned()
        .unwrap_or_default();

    let (text, post_mentioned_ids) = match msg_type {
        "text" => {
            let v: serde_json::Value = serde_json::from_str(content_str).ok()?;
            let t = v.get("text").and_then(|t| t.as_str()).filter(|s| !s.is_empty())?;
            (t.to_string(), vec![])
        }
        "post" => {
            let parsed = parse_post(content_str)?;
            (parsed.text, parsed.mentioned_open_ids)
        }
        _ => return None,
    };

    let text = strip_at_placeholders(&text);
    let text = text.trim().to_string();
    if text.is_empty() {
        return None;
    }

    if chat_type == "group"
        && !should_respond_in_group(mention_only, bot_open_id, &mentions, &post_mentioned_ids)
    {
        return None;
    }

    let timestamp = message
        .get("create_time")
        .and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok())
        .map(|ms| ms / 1000)
        .unwrap_or(0);

    Some(InboundEvent::Message(InboundMessage {
        message_id: msg_id.to_string(),
        chat_id: chat_id.to_string(),
        sender_id: sender_id.to_string(),
        sender_name: String::new(),
        text,
        attachments: vec![],
        timestamp,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── strip_at_placeholders ──

    #[test]
    fn strips_at_user_placeholder() {
        assert_eq!(strip_at_placeholders("hello @_user_1 world"), "hello world");
    }

    #[test]
    fn strips_multiple_placeholders() {
        assert_eq!(
            strip_at_placeholders("@_user_1 and @_user_23 here"),
            "and here"
        );
    }

    #[test]
    fn preserves_real_mentions() {
        assert_eq!(strip_at_placeholders("@Alice hello"), "@Alice hello");
    }

    // ── parse_post ──

    #[test]
    fn parse_post_extracts_text() {
        let content = serde_json::json!({
            "zh_cn": {
                "title": "Title",
                "content": [[{"tag": "text", "text": "Hello"}]]
            }
        });
        let result = parse_post(&content.to_string()).unwrap();
        assert!(result.text.contains("Hello"));
        assert!(result.text.contains("Title"));
    }

    #[test]
    fn parse_post_extracts_at_open_ids() {
        let content = serde_json::json!({
            "zh_cn": {
                "title": "",
                "content": [[{"tag": "at", "user_id": "ou_bot123", "user_name": "Bot"}]]
            }
        });
        let result = parse_post(&content.to_string()).unwrap();
        assert_eq!(result.mentioned_open_ids, vec!["ou_bot123"]);
    }

    #[test]
    fn parse_post_returns_none_for_empty() {
        assert!(parse_post("{}").is_none());
        assert!(parse_post("invalid").is_none());
    }

    // ── should_respond_in_group ──

    #[test]
    fn responds_when_mention_only_false() {
        assert!(should_respond_in_group(false, None, &[], &[]));
    }

    #[test]
    fn blocks_when_no_bot_open_id() {
        assert!(!should_respond_in_group(true, None, &[serde_json::json!({})], &[]));
    }

    #[test]
    fn responds_when_bot_mentioned() {
        let mentions = vec![serde_json::json!({"id": {"open_id": "ou_bot"}})];
        assert!(should_respond_in_group(true, Some("ou_bot"), &mentions, &[]));
    }

    #[test]
    fn blocks_when_other_mentioned() {
        let mentions = vec![serde_json::json!({"id": {"open_id": "ou_other"}})];
        assert!(!should_respond_in_group(true, Some("ou_bot"), &mentions, &[]));
    }

    #[test]
    fn responds_via_post_mentioned_ids() {
        assert!(should_respond_in_group(
            true,
            Some("ou_bot"),
            &[],
            &["ou_bot".to_string()]
        ));
    }

    // ── MessageDedup ──

    #[test]
    fn dedup_allows_new_message() {
        let mut dedup = MessageDedup::new();
        assert!(dedup.check_and_insert("msg_1"));
    }

    #[test]
    fn dedup_blocks_duplicate() {
        let mut dedup = MessageDedup::new();
        assert!(dedup.check_and_insert("msg_1"));
        assert!(!dedup.check_and_insert("msg_1"));
    }

    #[test]
    fn dedup_allows_different_messages() {
        let mut dedup = MessageDedup::new();
        assert!(dedup.check_and_insert("msg_1"));
        assert!(dedup.check_and_insert("msg_2"));
    }

    // ── parse_event ──

    fn make_text_event(sender: &str, chat_id: &str, text: &str) -> serde_json::Value {
        serde_json::json!({
            "sender": {"sender_id": {"open_id": sender}},
            "message": {
                "message_id": "om_1",
                "chat_id": chat_id,
                "chat_type": "p2p",
                "message_type": "text",
                "content": serde_json::json!({"text": text}).to_string(),
                "create_time": "1700000000000"
            }
        })
    }

    #[test]
    fn parse_event_text_message() {
        let event = make_text_event("ou_user", "oc_chat", "hello");
        let result = parse_event(&event, false, None).unwrap();
        if let InboundEvent::Message(m) = result {
            assert_eq!(m.text, "hello");
            assert_eq!(m.sender_id, "ou_user");
            assert_eq!(m.chat_id, "oc_chat");
            assert_eq!(m.timestamp, 1_700_000_000);
        } else {
            panic!("expected Message");
        }
    }

    #[test]
    fn parse_event_empty_text_returns_none() {
        let event = make_text_event("ou_user", "oc_chat", "  ");
        assert!(parse_event(&event, false, None).is_none());
    }

    #[test]
    fn parse_event_unsupported_type_returns_none() {
        let event = serde_json::json!({
            "sender": {"sender_id": {"open_id": "ou_user"}},
            "message": {
                "message_id": "om_1",
                "chat_id": "oc_chat",
                "chat_type": "p2p",
                "message_type": "image",
                "content": "{}",
            }
        });
        assert!(parse_event(&event, false, None).is_none());
    }

    #[test]
    fn parse_event_group_blocks_without_mention() {
        let event = serde_json::json!({
            "sender": {"sender_id": {"open_id": "ou_user"}},
            "message": {
                "message_id": "om_1",
                "chat_id": "oc_chat",
                "chat_type": "group",
                "message_type": "text",
                "content": serde_json::json!({"text": "hello"}).to_string(),
                "mentions": []
            }
        });
        assert!(parse_event(&event, true, Some("ou_bot")).is_none());
    }

    #[test]
    fn parse_event_group_allows_with_mention() {
        let event = serde_json::json!({
            "sender": {"sender_id": {"open_id": "ou_user"}},
            "message": {
                "message_id": "om_1",
                "chat_id": "oc_chat",
                "chat_type": "group",
                "message_type": "text",
                "content": serde_json::json!({"text": "hello"}).to_string(),
                "mentions": [{"id": {"open_id": "ou_bot"}}]
            }
        });
        assert!(parse_event(&event, true, Some("ou_bot")).is_some());
    }
}
