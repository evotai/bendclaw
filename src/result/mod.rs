//! Result: output formatting and event delivery.
//!
//! Transforms raw execution events into client-consumable formats:
//! plain text, JSON, NDJSON streaming, and SSE.
//!
//! Pipeline position: **fifth stage** — consumes `execution/` event stream.

use async_trait::async_trait;

use crate::base::Result;

/// Canonical contract: format and deliver an event stream to a consumer.
///
/// Implementations handle the specific wire format (text, JSON, SSE, etc.)
/// and delivery mechanism (HTTP response, channel message, CLI output).
#[async_trait]
pub trait EventStreamWriter: Send + Sync {
    type Event;
    type Sink;

    async fn write(&self, events: Vec<Self::Event>, sink: &mut Self::Sink) -> Result<()>;
}
