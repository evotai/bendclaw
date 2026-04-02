use crate::kernel::channels::model::account::ChannelAccount;
use crate::kernel::channels::model::message::InboundEvent;
use crate::kernel::channels::routing::dispatcher::ChannelDispatcher;

pub(crate) struct ValidatedInput {
    pub text: String,
    pub chat_id: String,
}

pub(crate) fn extract_and_validate(
    account: &ChannelAccount,
    event: &InboundEvent,
) -> Option<ValidatedInput> {
    let (text, reply_ctx) = ChannelDispatcher::extract_input(event);

    if let Some(sender_id) = event_sender_id(event) {
        if !is_sender_allowed(&account.config, sender_id) {
            return None;
        }
    }

    if text.trim().is_empty() {
        return None;
    }

    let chat_id = reply_ctx
        .as_ref()
        .map(|r| r.chat_id.clone())
        .unwrap_or_default();

    Some(ValidatedInput { text, chat_id })
}

/// Extract sender_id from any inbound event variant.
pub(crate) fn event_sender_id(event: &InboundEvent) -> Option<&str> {
    match event {
        InboundEvent::Message(msg) if !msg.sender_id.is_empty() => Some(&msg.sender_id),
        _ => None,
    }
}

pub(crate) fn event_message_id(event: &InboundEvent) -> Option<&str> {
    match event {
        InboundEvent::Message(msg) if !msg.message_id.is_empty() => Some(&msg.message_id),
        _ => None,
    }
}

/// Check if a sender is allowed by the account config's `allow_from` list.
/// - Missing or empty `allow_from` -> allow all (backward compatible).
/// - `"*"` in the list -> allow all.
/// - Otherwise sender_id must match one of the entries.
pub fn is_sender_allowed(config: &serde_json::Value, sender_id: &str) -> bool {
    let Some(list) = config.get("allow_from").and_then(|v| v.as_array()) else {
        return true;
    };
    if list.is_empty() {
        return true;
    }
    list.iter().any(|entry| {
        let s = entry.as_str().unwrap_or("");
        s == "*" || s == sender_id || s.split('|').any(|part| part == sender_id)
    })
}
