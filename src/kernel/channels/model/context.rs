//! Channel context — parsed from conversation base keys for channel-aware tools.

/// Channel context extracted from a conversation base key.
/// Channel base keys follow the format: `{channel_type}:{account_id}:{chat_id}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelContext {
    pub channel_type: String,
    pub account_id: String,
    pub chat_id: String,
}

impl ChannelContext {
    pub fn base_key(channel_type: &str, account_id: &str, chat_id: &str) -> String {
        format!("{channel_type}:{account_id}:{chat_id}")
    }

    /// Parse a conversation base key into channel context.
    pub fn from_base_key(base_key: &str) -> Option<Self> {
        let parts: Vec<&str> = base_key.splitn(3, ':').collect();
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
