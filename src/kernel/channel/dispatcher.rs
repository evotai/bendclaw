use std::sync::Arc;

use crate::kernel::session::session_manager::SessionManager;

use super::message::InboundEvent;
use super::message::ReplyContext;
use super::registry::ChannelRegistry;

/// Result of dispatching an inbound event.
pub struct DispatchResult {
    pub run_id: String,
    pub session_id: String,
    pub reply_context: Option<ReplyContext>,
}

/// Unified message dispatcher. All channel inbound events flow through here.
/// Analogous to moltis ChatService::send().
pub struct ChannelDispatcher {
    registry: Arc<ChannelRegistry>,
    #[allow(dead_code)]
    sessions: Arc<SessionManager>,
}

impl ChannelDispatcher {
    pub fn new(
        registry: Arc<ChannelRegistry>,
        sessions: Arc<SessionManager>,
    ) -> Self {
        Self {
            registry,
            sessions,
        }
    }

    pub fn registry(&self) -> &Arc<ChannelRegistry> {
        &self.registry
    }

    /// Build a session key that uniquely identifies a conversation across channels.
    pub fn session_key(channel_type: &str, account_id: &str, chat_id: &str) -> String {
        format!("{channel_type}:{account_id}:{chat_id}")
    }

    /// Extract the user-facing text and reply context from any inbound event.
    pub fn extract_input(event: &InboundEvent) -> (String, Option<ReplyContext>) {
        match event {
            InboundEvent::Message(msg) => {
                let reply_ctx = ReplyContext {
                    chat_id: msg.chat_id.clone(),
                    reply_to_message_id: Some(msg.message_id.clone()),
                    thread_id: None,
                };
                (msg.text.clone(), Some(reply_ctx))
            }
            InboundEvent::PlatformEvent {
                event_type,
                payload,
                reply_context,
            } => {
                let summary = payload
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| {
                        serde_json::to_string(payload).unwrap_or_default()
                    });
                let input = format!("[{event_type}] {summary}");
                (input, reply_context.clone())
            }
            InboundEvent::Callback {
                data,
                reply_context,
                ..
            } => (data.clone(), reply_context.clone()),
        }
    }
}
