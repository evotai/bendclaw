use crate::app::result::event_envelope::EventEnvelope;

/// Subscribes to EventEnvelope stream and emits semantic logs.
pub fn observe_event(envelope: &EventEnvelope) {
    tracing::debug!(
        target: "bendclaw::observer",
        seq = envelope.sequence,
        event = %envelope.event_name,
        session_id = %envelope.session_id,
        run_id = %envelope.run_id,
        "run_event"
    );
}
