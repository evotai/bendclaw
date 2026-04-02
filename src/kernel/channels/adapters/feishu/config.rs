use serde::Deserialize;
use serde::Serialize;

pub const FEISHU_CHANNEL_TYPE: &str = "feishu";
pub const FEISHU_API: &str = "https://open.feishu.cn/open-apis";
pub const FEISHU_DOMAIN: &str = "https://open.feishu.cn";
pub const FEISHU_MAX_MESSAGE_LEN: usize = 30_000;

pub const DEFAULT_PING_INTERVAL_SECS: u64 = 120;

// ── Endpoint error codes ──
// 0 = success
pub const ENDPOINT_OK: i64 = 0;
// Client errors — stop reconnecting
pub const ENDPOINT_AUTH_FAILED: i64 = 1;
pub const ENDPOINT_FORBIDDEN: i64 = 403;
pub const ENDPOINT_CONN_LIMIT: i64 = 1000040350;
pub const ENDPOINT_APP_INACTIVE: i64 = 1000040343;
// Server errors — keep reconnecting
#[allow(dead_code)]
pub const ENDPOINT_BUSY: i64 = 514;

/// Returns true if the endpoint error code indicates a client error
/// (authentication failure, connection limit, etc.) where reconnecting is futile.
pub fn is_client_error(code: i64) -> bool {
    matches!(
        code,
        ENDPOINT_AUTH_FAILED | ENDPOINT_FORBIDDEN | ENDPOINT_CONN_LIMIT | ENDPOINT_APP_INACTIVE
    )
}

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub allow_from: Vec<String>,
    /// In group chats, only respond to messages that @mention the bot.
    /// Defaults to true.
    #[serde(default = "default_mention_only")]
    pub mention_only: bool,
}

fn default_mention_only() -> bool {
    true
}

// ── ReconnectConfig ──

/// Server-provided reconnect parameters from endpoint response and pong frames.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Max reconnect attempts before giving up. 0 = unlimited.
    pub reconnect_count: u64,
    /// Base reconnect interval in seconds.
    pub reconnect_interval: u64,
    /// Random nonce (0..nonce) added to first reconnect delay.
    pub reconnect_nonce: u64,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            reconnect_count: 0,
            reconnect_interval: 5,
            reconnect_nonce: 0,
        }
    }
}

impl ReconnectConfig {
    /// Parse from endpoint response JSON `data.ClientConfig`.
    pub fn from_client_config(config: &serde_json::Value) -> Self {
        Self {
            reconnect_count: config["ReconnectCount"].as_u64().unwrap_or(0),
            reconnect_interval: config["ReconnectInterval"].as_u64().unwrap_or(5).max(1),
            reconnect_nonce: config["ReconnectNonce"].as_u64().unwrap_or(0),
        }
    }

    /// Update from pong payload (may contain updated reconnect params).
    pub fn update_from_pong(&mut self, payload: &serde_json::Value) {
        if let Some(v) = payload["ReconnectCount"].as_u64() {
            self.reconnect_count = v;
        }
        if let Some(v) = payload["ReconnectInterval"].as_u64() {
            if v > 0 {
                self.reconnect_interval = v;
            }
        }
        if let Some(v) = payload["ReconnectNonce"].as_u64() {
            self.reconnect_nonce = v;
        }
    }
}

// ── Sender allow-list ──

pub fn is_sender_allowed(allow_from: &[String], sender_id: &str) -> bool {
    if allow_from.is_empty() {
        return true;
    }
    if allow_from.iter().any(|s| s == "*") {
        return true;
    }
    allow_from.iter().any(|s| s == sender_id)
}
