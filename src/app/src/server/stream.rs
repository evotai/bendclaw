use std::convert::Infallible;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::error::BendclawError;
use crate::error::Result;
use crate::run::payload_as;
use crate::run::AssistantBlock;
use crate::run::AssistantPayload;
use crate::run::EventSink;
use crate::run::MessagePayload;
use crate::run::RunFinishedPayload;
use crate::run::ToolResultPayload;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;

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

pub fn map_run_event(run_event: &RunEvent) -> Vec<SseEvent> {
    let mut events = Vec::new();

    match &run_event.kind {
        RunEventKind::AssistantMessage => {
            if let Some(payload) = payload_as::<AssistantPayload>(&run_event.payload) {
                for block in payload.content {
                    match block {
                        AssistantBlock::Text { text } if !text.is_empty() => {
                            events.push(event("text", &json!({ "text": text })));
                        }
                        AssistantBlock::ToolUse { id, name, input } => {
                            events.push(event(
                                "tool_use",
                                &json!({ "id": id, "name": name, "input": input }),
                            ));
                        }
                        AssistantBlock::Thinking { text } if !text.is_empty() => {
                            events.push(event("thinking", &json!({ "thinking": text })));
                        }
                        _ => {}
                    }
                }
            }
        }
        RunEventKind::ToolResult => {
            if let Some(payload) = payload_as::<ToolResultPayload>(&run_event.payload) {
                events.push(event(
                    "tool_result",
                    &json!({
                        "tool_use_id": payload.tool_use_id,
                        "content": payload.content,
                        "is_error": payload.is_error,
                    }),
                ));
            }
        }
        RunEventKind::RunFinished => {
            if let Some(payload) = payload_as::<RunFinishedPayload>(&run_event.payload) {
                let input_tokens = payload
                    .usage
                    .get("input_tokens")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default();
                let output_tokens = payload
                    .usage
                    .get("output_tokens")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default();
                events.push(event(
                    "result",
                    &json!({
                        "num_turns": payload.num_turns,
                        "input_tokens": input_tokens,
                        "output_tokens": output_tokens,
                        "cost": payload.cost_usd,
                        "duration_ms": payload.duration_ms,
                    }),
                ));
            }
        }
        RunEventKind::Error => {
            if let Some(payload) = payload_as::<MessagePayload>(&run_event.payload) {
                events.push(error_event(payload.message));
            }
        }
        RunEventKind::PartialMessage => {
            if let Some(payload) = payload_as::<MessagePayload>(&run_event.payload) {
                if !payload.message.is_empty() {
                    events.push(event("text", &json!({ "text": payload.message })));
                }
            }
        }
        _ => {}
    }

    events
}

fn event(event_type: &str, data: &serde_json::Value) -> SseEvent {
    let payload = json!({ "type": event_type, "data": data });
    match serde_json::to_string(&payload) {
        Ok(json) => Ok(axum::response::sse::Event::default().data(json)),
        Err(_) => Ok(axum::response::sse::Event::default().data(String::new())),
    }
}
