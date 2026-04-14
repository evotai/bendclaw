use std::collections::HashMap;

use regex::Regex;

use super::config::FeishuChannelConfig;

// ── At-placeholder stripping ──

/// Remove `@_user_N` placeholders injected by Feishu for @mentions.
pub fn strip_at_placeholders(text: &str) -> String {
    static RE: std::sync::LazyLock<Option<Regex>> =
        std::sync::LazyLock::new(|| Regex::new(r"@_user_\d+").ok());
    match RE.as_ref() {
        Some(re) => re.replace_all(text, "").trim().to_string(),
        None => text.trim().to_string(),
    }
}

// ── Parsed message ──

pub struct ParsedMessage {
    pub message_id: String,
    pub chat_id: String,
    pub sender_id: String,
    pub text: String,
}

// ── Post (rich text) parsing ──

struct ParsedPost {
    text: String,
    mentioned_open_ids: Vec<String>,
}

fn parse_post(content: &serde_json::Value) -> Option<ParsedPost> {
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
                "text" | "a" => {
                    if let Some(t) = elem.get("text").and_then(|v| v.as_str()) {
                        line_parts.push(t.to_string());
                    }
                }
                "at" => {
                    if let Some(uid) = elem.get("user_id").and_then(|v| v.as_str()) {
                        mentioned_open_ids.push(uid.to_string());
                    }
                    if let Some(name) = elem.get("user_name").and_then(|v| v.as_str()) {
                        line_parts.push(format!("@{name}"));
                    }
                }
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

fn should_respond_in_group(
    mention_only: bool,
    bot_open_id: &str,
    mentions: &[serde_json::Value],
    post_mentioned_ids: &[String],
) -> bool {
    if !mention_only {
        return true;
    }
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
    post_mentioned_ids.iter().any(|id| id == bot_open_id)
}

// ── Message dedup ──

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

    pub fn check_and_insert(&mut self, msg_id: &str) -> bool {
        let now = std::time::Instant::now();
        self.seen
            .retain(|_, t| now.duration_since(*t) < self.window);
        if self.seen.contains_key(msg_id) {
            return false;
        }
        self.seen.insert(msg_id.to_string(), now);
        true
    }
}

// ── Sender allow-list ──

fn is_sender_allowed(allow_from: &[String], sender_id: &str) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    if allow_from.iter().any(|s| s == "*") {
        return true;
    }
    allow_from.iter().any(|s| s == sender_id)
}

// ── Unified event parser ──

pub fn parse_event(
    event_json: &serde_json::Value,
    config: &FeishuChannelConfig,
    bot_open_id: &str,
    dedup: &mut MessageDedup,
) -> Option<ParsedMessage> {
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

    if !is_sender_allowed(&config.allow_from, sender_id) {
        tracing::debug!(channel = "feishu", sender_id, "sender denied by allow_from");
        return None;
    }

    if !dedup.check_and_insert(msg_id) {
        return None;
    }

    let content_str = message.get("content")?.as_str()?;
    let content: serde_json::Value = match serde_json::from_str(content_str) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(channel = "feishu", msg_id, error = %e, "failed to parse content");
            return None;
        }
    };

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
            tracing::debug!(
                channel = "feishu",
                msg_type,
                msg_id,
                "unsupported message type"
            );
            return None;
        }
    };

    if chat_type == "group" {
        let mentions = message
            .get("mentions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        if !should_respond_in_group(
            config.mention_only,
            bot_open_id,
            &mentions,
            &post_mentioned_ids,
        ) {
            return None;
        }
    }

    let text = strip_at_placeholders(&text);
    if text.is_empty() {
        return None;
    }

    Some(ParsedMessage {
        message_id: msg_id.to_string(),
        chat_id: chat_id.to_string(),
        sender_id: sender_id.to_string(),
        text,
    })
}
