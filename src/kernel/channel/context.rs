//! Channel context — parsed from session keys for channel-aware tools.

/// Channel context extracted from a session key.
/// Session keys from channel dispatchers follow the format: `{channel_type}:{account_id}:{chat_id}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelContext {
    pub channel_type: String,
    pub account_id: String,
    pub chat_id: String,
}

impl ChannelContext {
    /// Parse a session key into channel context.
    /// Returns `None` for non-channel sessions (e.g. API sessions like "s1").
    /// Handles optional `#timestamp` suffix: `feishu:acct:chat#1711180800`
    pub fn from_session_key(session_key: &str) -> Option<Self> {
        // Strip optional #suffix (session generation marker)
        let base = session_key.split('#').next().unwrap_or(session_key);
        let parts: Vec<&str> = base.splitn(3, ':').collect();
        if parts.len() == 3 && parts.iter().all(|p| !p.is_empty()) {
            Some(Self {
                channel_type: parts[0].to_string(),
                account_id: parts[1].to_string(),
                chat_id: parts[2].to_string(),
            })
        } else {
            None
        }
    }
}
