use std::time::Duration;

use tokio::sync::mpsc;

use crate::channels::model::account::ChannelAccount;
use crate::channels::model::message::InboundEvent;
use crate::channels::routing::dispatcher::ChannelDispatcher;

pub struct DebounceConfig {
    pub window: Duration,
    pub max_wait: Duration,
}

impl Default for DebounceConfig {
    fn default() -> Self {
        Self {
            window: Duration::from_millis(500),
            max_wait: Duration::from_secs(2),
        }
    }
}

/// A job queued in a per-chat serial queue.
pub struct ChatJob {
    pub account: ChannelAccount,
    pub event: InboundEvent,
}

/// Result of debouncing: merged text from one or more rapid messages.
pub struct DebouncedInput {
    pub account: ChannelAccount,
    pub text: String,
    pub primary_event: InboundEvent,
    /// All events that were merged (including the primary). Used for dedup and persistence.
    pub all_events: Vec<InboundEvent>,
    pub merged_count: usize,
}

/// Debounce may produce a leftover job from a different sender.
pub enum DebounceResult {
    Ready(DebouncedInput),
    ReadyWithLeftover(Box<DebouncedInput>, ChatJob),
}

/// Debounce rapid consecutive messages from the same sender.
///
/// Control commands (`/new`, `/clear`, `/cancel`, `/status`, `/stop`, `/abort`)
/// bypass debounce entirely.
pub async fn debounce(
    config: &DebounceConfig,
    first: ChatJob,
    rx: &mut mpsc::Receiver<ChatJob>,
) -> DebounceResult {
    let (text, _reply_ctx) = ChannelDispatcher::extract_input(&first.event);
    let sender_id = event_sender_id(&first.event).map(|s| s.to_string());

    // Control commands skip debounce.
    if is_control_command(text.trim()) {
        return DebounceResult::Ready(DebouncedInput {
            account: first.account,
            text,
            primary_event: first.event.clone(),
            all_events: vec![first.event],
            merged_count: 1,
        });
    }

    let mut merged_text = text;
    let mut merged_count: usize = 1;
    let mut all_events = vec![first.event.clone()];
    let deadline = tokio::time::Instant::now() + config.max_wait;

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let wait = config.window.min(remaining);

        match tokio::time::timeout(wait, rx.recv()).await {
            // Timeout — no more messages in window.
            Err(_) => break,
            // Channel closed.
            Ok(None) => break,
            Ok(Some(next)) => {
                let next_sender = event_sender_id(&next.event).map(|s| s.to_string());

                // Different sender → return leftover for caller to re-process.
                if next_sender != sender_id {
                    return DebounceResult::ReadyWithLeftover(
                        Box::new(DebouncedInput {
                            account: first.account,
                            text: merged_text,
                            primary_event: first.event,
                            all_events,
                            merged_count,
                        }),
                        next,
                    );
                }

                // Same sender → merge.
                let (next_text, _) = ChannelDispatcher::extract_input(&next.event);
                if !next_text.trim().is_empty() {
                    merged_text.push('\n');
                    merged_text.push_str(&next_text);
                    merged_count += 1;
                }
                all_events.push(next.event);
            }
        }
    }

    DebounceResult::Ready(DebouncedInput {
        account: first.account,
        text: merged_text,
        primary_event: first.event,
        all_events,
        merged_count,
    })
}

fn is_control_command(input: &str) -> bool {
    matches!(
        input,
        "/new" | "/clear" | "/cancel" | "/status" | "/stop" | "/abort"
    )
}

fn event_sender_id(event: &InboundEvent) -> Option<&str> {
    match event {
        InboundEvent::Message(msg) if !msg.sender_id.is_empty() => Some(&msg.sender_id),
        _ => None,
    }
}
