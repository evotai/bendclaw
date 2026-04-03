use crate::result::event_envelope::EventEnvelope;

/// Collect a stream of envelopes into aggregated JSON.
pub fn collect_json(envelopes: &[EventEnvelope]) -> serde_json::Value {
    serde_json::to_value(envelopes).unwrap_or(serde_json::Value::Null)
}
