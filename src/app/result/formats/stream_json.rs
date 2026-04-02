use crate::app::result::event_envelope::EventEnvelope;

/// Encode a single envelope as a JSON line.
pub fn encode(envelope: &EventEnvelope) -> String {
    serde_json::to_string(envelope).unwrap_or_else(|_| "{}".to_string())
}
