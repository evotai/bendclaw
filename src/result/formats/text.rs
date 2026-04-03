use crate::result::event_envelope::EventEnvelope;

/// Collect a stream of envelopes into aggregated text.
/// Extracts text content from assistant.output events, concatenates the rest
/// as event names for non-output events.
pub fn collect_text(envelopes: &[EventEnvelope]) -> String {
    let mut parts = Vec::new();
    for env in envelopes {
        if env.event_name == "assistant.output" {
            if let Some(text) = env.payload.get("text").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        }
    }
    if parts.is_empty() {
        for env in envelopes {
            if let Some(text) = env.payload.get("prompt").and_then(|v| v.as_str()) {
                parts.push(text.to_string());
            }
        }
    }
    parts.join("")
}
