use super::event_envelope::EventEnvelope;
use crate::types::entities::RunEvent;

/// Map a kernel RunEvent into an EventEnvelope.
pub fn map_run_event(event: &RunEvent) -> EventEnvelope {
    EventEnvelope {
        sequence: event.seq as u64,
        timestamp: event.created_at.clone(),
        session_id: event.session_id.clone(),
        run_id: event.run_id.clone(),
        event_name: event.kind.as_str().to_string(),
        payload: event.payload.clone(),
        cursor: None,
    }
}
