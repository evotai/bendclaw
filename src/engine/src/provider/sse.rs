//! Shared SSE (Server-Sent Events) types.
//!
//! The actual SSE parsing from HTTP responses lives in [`super::stream_http`].
//! This module provides the shared [`SseEvent`] type used across providers.

/// A parsed SSE event with event type and data.
#[derive(Debug, Clone)]
pub struct SseEvent {
    pub event: String,
    pub data: String,
}
