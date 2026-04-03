use crate::result::event_envelope::EventEnvelope;

/// Encode a single envelope as an SSE event.
pub fn encode(envelope: &EventEnvelope) -> String {
    let data = serde_json::to_string(envelope).unwrap_or_else(|_| "{}".to_string());
    format!("event: {}\ndata: {}\n\n", envelope.event_name, data)
}
