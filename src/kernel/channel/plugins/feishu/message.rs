use std::collections::HashMap;

use regex::Regex;

use super::config::is_sender_allowed;
use super::config::FeishuConfig;
use crate::kernel::channel::message::InboundEvent;
use crate::kernel::channel::message::InboundMessage;
use crate::observability::log::slog;

// ── At-placeholder stripping ──

/// Remove `@_user_N` placeholders injected by Feishu for @mentions.
pub fn strip_at_placeholders(text: &str) -> String {
    let re = Regex::new(r"@_user_\d+").unwrap_or_else(|_| Regex::new(r"$^").unwrap());
    re.replace_all(text, "").trim().to_string()
}

// ── Post (rich text) parsing ──

pub struct ParsedPost {
    pub text: String,
    pub mentioned_open_ids: Vec<String>,
}

/// Flatten a Feishu post (rich text) content into plain text.
/// Extracts mentioned open_ids from `at` tags.
pub fn parse_post(content: &serde_json::Value) -> Option<ParsedPost> {
    // Post content structure: {"title":"...", "content":[[{tag,text,...}]]}
    let paragraphs = content.get("content")?.as_array()?;
    let mut text_parts = Vec::new();
    let mut mentioned_open_ids = Vec::new();

    if let Some(title) = content.get("title").and_then(|v| v.as_str()) {
        if !title.is_empty() {
            text_parts.push(title.to_string());
        }
    }

    for paragraph in paragraphs {
        let elements = match paragraph.as_array() {
            Some(a) => a,
            None => continue,
        };
        let mut line_parts = Vec::new();
        for elem in elements {
            let tag = elem.get("tag").and_then(|v| v.as_str()).unwrap_or("");
            match tag {
                "text" => {
                    if let Some(t) = elem.get("text").and_then(|v| v.as_str()) {
                        line_parts.push(t.to_string());
                    }
                }
                "a" => {
                    if let Some(t) = elem.get("text").and_then(|v| v.as_str()) {
                        line_parts.push(t.to_string());
                    }
                }
                "at" => {
                    if let Some(uid) = elem.get("user_id").and_then(|v| v.as_str()) {
                        mentioned_open_ids.push(uid.to_string());
                    }
                    // at tags also have a user_name we can include
                    if let Some(name) = elem.get("user_name").and_then(|v| v.as_str()) {
                        line_parts.push(format!("@{name}"));
                    }
                }
                // img, media, emotion — skip silently
                _ => {}
            }
        }
        if !line_parts.is_empty() {
            text_parts.push(line_parts.join(""));
        }
    }

    let text = text_parts.join("\n").trim().to_string();
    if text.is_empty() {
        return None;
    }
    Some(ParsedPost {
        text,
        mentioned_open_ids,
    })
}

// ── Group mention filter ──

/// Decide whether the bot should respond to a group message.
/// If `mention_only` is true, only respond when the bot is @mentioned.
pub fn should_respond_in_group(
    mention_only: bool,
    bot_open_id: &str,
    mentions: &[serde_json::Value],
    post_mentioned_ids: &[String],
) -> bool {
    if !mention_only {
        return true;
    }
    // Check standard mentions array (from message.mentions)
    for m in mentions {
        if let Some(id) = m
            .get("id")
            .and_then(|v| v.get("open_id"))
            .and_then(|v| v.as_str())
        {
            if id == bot_open_id {
                return true;
            }
        }
    }
    // Check post at-tag mentions
    post_mentioned_ids.iter().any(|id| id == bot_open_id)
}

// ── Message dedup ──

/// Simple message deduplication with a time window.
pub struct MessageDedup {
    seen: HashMap<String, std::time::Instant>,
    window: std::time::Duration,
}

impl MessageDedup {
    pub fn new(window: std::time::Duration) -> Self {
        Self {
            seen: HashMap::new(),
            window,
        }
    }

    /// Returns true if this message_id is new (not a duplicate).
    /// Cleans up expired entries on each call.
    pub fn check_and_insert(&mut self, msg_id: &str) -> bool {
        let now = std::time::Instant::now();
        // Cleanup expired
        self.seen
            .retain(|_, t| now.duration_since(*t) < self.window);
        if self.seen.contains_key(msg_id) {
            return false;
        }
        self.seen.insert(msg_id.to_string(), now);
        true
    }
}

// ── Unified event parser ──

/// Parse a Feishu event payload into an InboundEvent.
/// Handles both `text` and `post` message types.
/// Returns None for unsupported types or filtered-out messages.
pub fn parse_event(
    event_json: &serde_json::Value,
    config: &FeishuConfig,
    dedup: &mut MessageDedup,
) -> Option<InboundEvent> {
    let event_type = event_json
        .pointer("/header/event_type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if event_type != "im.message.receive_v1" {
        return None;
    }

    let event_data = event_json.get("event")?;
    let message = event_data.get("message")?;
    let sender = event_data.get("sender")?.get("sender_id")?;

    let msg_id = message.get("message_id")?.as_str()?;
    let chat_id = message.get("chat_id")?.as_str()?;
    let sender_id = sender.get("open_id")?.as_str()?;
    let msg_type = message.get("message_type")?.as_str()?;
    let chat_type = message
        .get("chat_type")
        .and_then(|v| v.as_str())
        .unwrap_or("p2p");

    // Sender allow-list check
    if !is_sender_allowed(&config.allow_from, sender_id) {
        slog!(warn, "feishu_ws", "sender_denied", sender_id,);
        return None;
    }

    // Dedup check
    if !dedup.check_and_insert(msg_id) {
        return None;
    }

    let content_str = message.get("content")?.as_str()?;
    let content: serde_json::Value = match serde_json::from_str(content_str) {
        Ok(v) => v,
        Err(e) => {
            slog!(warn, "feishu_ws", "content_parse_failed", msg_id, error = %e,);
            return None;
        }
    };

    // Parse text based on message type
    let (text, post_mentioned_ids) = match msg_type {
        "text" => {
            let t = content.get("text").and_then(|v| v.as_str())?;
            (t.to_string(), Vec::new())
        }
        "post" => {
            let parsed = parse_post(&content)?;
            (parsed.text, parsed.mentioned_open_ids)
        }
        _ => {
            slog!(warn, "feishu_ws", "unsupported_msg_type", msg_type, msg_id,);
            return None;
        }
    };

    // Group mention filter
    if chat_type == "group" {
        let mentions = message
            .get("mentions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        // bot_open_id: Feishu puts it in the app_id field of mentions
        // We use app_id from config as a fallback identifier
        let bot_open_id = mentions
            .iter()
            .find_map(|m| {
                let key = m.get("key").and_then(|v| v.as_str()).unwrap_or("");
                if key.starts_with("@_user_") {
                    m.get("id")
                        .and_then(|v| v.get("open_id"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
            .unwrap_or_default();

        if !should_respond_in_group(
            config.mention_only,
            &bot_open_id,
            &mentions,
            &post_mentioned_ids,
        ) {
            return None;
        }
    }

    // Strip @placeholders from final text
    let text = strip_at_placeholders(&text);
    if text.is_empty() {
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
