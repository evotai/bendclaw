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
    pub fn from_session_key(session_key: &str) -> Option<Self> {
        let parts: Vec<&str> = session_key.splitn(3, ':').collect();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_feishu_session() {
        let ctx =
            ChannelContext::from_session_key("feishu:01kkzesy9x1b0x4rt4j57mzevw:oc_6406fca8d3a2")
                .unwrap();
        assert_eq!(ctx.channel_type, "feishu");
        assert_eq!(ctx.account_id, "01kkzesy9x1b0x4rt4j57mzevw");
        assert_eq!(ctx.chat_id, "oc_6406fca8d3a2");
    }

    #[test]
    fn parse_telegram_session() {
        let ctx = ChannelContext::from_session_key("telegram:bot123:chat456").unwrap();
        assert_eq!(ctx.channel_type, "telegram");
        assert_eq!(ctx.account_id, "bot123");
        assert_eq!(ctx.chat_id, "chat456");
    }

    #[test]
    fn parse_github_session() {
        let ctx = ChannelContext::from_session_key("github:app42:issue_789").unwrap();
        assert_eq!(ctx.channel_type, "github");
        assert_eq!(ctx.account_id, "app42");
        assert_eq!(ctx.chat_id, "issue_789");
    }

    #[test]
    fn parse_http_api_session() {
        let ctx = ChannelContext::from_session_key("http_api:acc1:conv99").unwrap();
        assert_eq!(ctx.channel_type, "http_api");
        assert_eq!(ctx.account_id, "acc1");
        assert_eq!(ctx.chat_id, "conv99");
    }

    #[test]
    fn chat_id_with_colons_preserved() {
        let ctx = ChannelContext::from_session_key("feishu:acc:chat:with:colons").unwrap();
        assert_eq!(ctx.channel_type, "feishu");
        assert_eq!(ctx.account_id, "acc");
        assert_eq!(ctx.chat_id, "chat:with:colons");
    }

    #[test]
    fn non_channel_session_returns_none() {
        assert!(ChannelContext::from_session_key("s1").is_none());
        assert!(ChannelContext::from_session_key("").is_none());
        assert!(ChannelContext::from_session_key("only:two").is_none());
    }

    #[test]
    fn empty_parts_return_none() {
        assert!(ChannelContext::from_session_key("::").is_none());
        assert!(ChannelContext::from_session_key("feishu::chat").is_none());
        assert!(ChannelContext::from_session_key("feishu:acc:").is_none());
    }
}
