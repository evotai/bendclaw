//! Result: output formatting and event delivery.
//!
//! Transforms raw execution events into client-consumable formats:
//! plain text, JSON, NDJSON streaming, and SSE.
//!
//! Pipeline position: **fifth stage** — consumes `execution/` event stream.

pub mod event_cursor;
pub mod event_envelope;
pub mod formats;
pub mod result_format;
pub mod run_event_mapper;

use async_trait::async_trait;
pub use event_cursor::EventCursor;
pub use event_envelope::EventEnvelope;
pub use result_format::ResultFormat;
pub use run_event_mapper::map_run_event;

use crate::types::Result;

/// Canonical contract: format and deliver an event stream to a consumer.
#[async_trait]
pub trait EventStreamWriter: Send + Sync {
    type Event;
    type Sink;

    async fn write(&self, events: Vec<Self::Event>, sink: &mut Self::Sink) -> Result<()>;
}
