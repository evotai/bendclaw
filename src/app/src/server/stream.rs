use std::convert::Infallible;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::AssistantBlock;
use crate::protocol::RunEvent;
use crate::protocol::RunEventPayload;
use crate::request::EventSink;

pub type SseEvent = std::result::Result<axum::response::sse::Event, Infallible>;

pub(crate) struct SseSink {
    tx: tokio::sync::mpsc::Sender<SseEvent>,
}

impl SseSink {
    pub(crate) fn new(tx: tokio::sync::mpsc::Sender<SseEvent>) -> Self {
        Self { tx }
    }
}

#[async_trait]
impl EventSink for SseSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        for item in map_run_event(event.as_ref()) {
            self.tx
                .send(item)
                .await
                .map_err(|e| BendclawError::Run(format!("failed to publish server event: {e}")))?;
        }
        Ok(())
    }
}

pub fn done_event() -> SseEvent {
    event("done", &json!(null))
}

pub fn error_event(message: impl Into<String>) -> SseEvent {
    event("error", &json!({ "message": message.into() }))
}

/// Map a RunEvent to a list of SSE JSON payloads (stable, testable).
/// Each returned Value has shape: { "type": "...", "data": {...} }
pub fn map_run_event_json(run_event: &RunEvent) -> Vec<serde_json::Value> {
    let mut events = Vec::new();

    match &run_event.payload {
        RunEventPayload::AssistantCompleted { content, .. } => {
            for block in content {
                match block {
                    AssistantBlock::Text { .. } => {}
                    AssistantBlock::ToolCall { id, name, input } => {
                        events.push(json!({
                            "type": "tool_call",
                            "data": { "id": id, "name": name, "input": input }
                        }));
                    }
                    AssistantBlock::Thinking { text } if !text.is_empty() => {
                        events.push(json!({ "type": "thinking", "data": { "thinking": text } }));
                    }
                    _ => {}
                }
            }
        }
        RunEventPayload::ToolFinished {
            tool_call_id,
            content,
            is_error,
            ..
        } => {
            events.push(json!({
                "type": "tool_result",
                "data": {
                    "tool_call_id": tool_call_id,
                    "content": content,
                    "is_error": is_error,
                }
            }));
        }
        RunEventPayload::RunFinished {
            turn_count,
            usage,
            duration_ms,
            ..
        } => {
            events.push(json!({
                "type": "result",
                "data": {
                    "turn_count": turn_count,
                    "input_tokens": usage.input,
                    "output_tokens": usage.output,
                    "duration_ms": duration_ms,
                }
            }));
        }
        RunEventPayload::Error { message } => {
            events.push(json!({ "type": "error", "data": { "message": message } }));
        }
        RunEventPayload::AssistantDelta { delta, .. } => {
            if let Some(delta) = delta {
                if !delta.is_empty() {
                    events.push(json!({ "type": "text", "data": { "text": delta } }));
                }
            }
        }
        _ => {}
    }

    events
}

pub fn map_run_event(run_event: &RunEvent) -> Vec<SseEvent> {
    map_run_event_json(run_event)
        .iter()
        .map(|payload| match serde_json::to_string(payload) {
            Ok(json) => Ok(axum::response::sse::Event::default().data(json)),
            Err(_) => Ok(axum::response::sse::Event::default().data(String::new())),
        })
        .collect()
}

fn event(event_type: &str, data: &serde_json::Value) -> SseEvent {
    let payload = json!({ "type": event_type, "data": data });
    match serde_json::to_string(&payload) {
        Ok(json) => Ok(axum::response::sse::Event::default().data(json)),
        Err(_) => Ok(axum::response::sse::Event::default().data(String::new())),
    }
}
