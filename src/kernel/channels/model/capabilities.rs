use std::time::Duration;

/// The fundamental nature of a channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelKind {
    /// User sends message → agent replies (Telegram, Feishu, HTTP API).
    Conversational,
    /// Platform events → trigger agent behavior (GitHub, GitLab).
    EventDriven,
    /// Both conversation and events (Slack).
    Hybrid,
}

/// How inbound messages arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundMode {
    /// Synchronous request-response (HTTP API).
    HttpRequest,
    /// Async webhook callbacks (GitHub).
    Webhook,
    /// Long polling.
    Polling,
    /// Persistent connection (Discord, Slack Socket Mode).
    WebSocket,
}

/// Declares what a channel implementation supports.
#[derive(Debug, Clone)]
pub struct ChannelCapabilities {
    pub channel_kind: ChannelKind,
    pub inbound_mode: InboundMode,
    /// Edit-in-place streaming (Telegram, Slack).
    pub supports_edit: bool,
    /// SSE or similar (HTTP API).
    pub supports_streaming: bool,
    pub supports_markdown: bool,
    /// Thread replies (Slack, Discord).
    pub supports_threads: bool,
    /// Emoji reactions (GitHub, Slack).
    pub supports_reactions: bool,
    pub max_message_len: usize,
    pub stale_event_threshold: Option<Duration>,
}
