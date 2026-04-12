use std::convert::Infallible;

use serde_json::json;

use crate::agent::AssistantBlock;
use crate::agent::RunEvent;
use crate::agent::RunEventPayload;

pub type SseEvent = std::result::Result<axum::response::sse::Event, Infallible>;

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
        RunEventPayload::AssistantDelta {
            delta: Some(delta), ..
        } => {
            if !delta.is_empty() {
                events.push(json!({ "type": "text", "data": { "text": delta } }));
            }
        }
        RunEventPayload::LlmCallStarted {
            turn,
            attempt,
            model,
            system_prompt,
            messages,
            tools,
            message_count,
            message_bytes,
            system_prompt_tokens,
        } => {
            events.push(json!({
                "type": "llm_call_started",
                "data": {
                    "turn": turn,
                    "attempt": attempt,
                    "model": model,
                    "system_prompt": system_prompt,
                    "messages": messages,
                    "tools": tools,
                    "message_count": message_count,
                    "message_bytes": message_bytes,
                    "system_prompt_tokens": system_prompt_tokens,
                }
            }));
        }
        RunEventPayload::LlmCallCompleted {
            turn,
            attempt,
            usage,
            error,
            metrics,
            ..
        } => {
            let mut data = json!({
                "turn": turn,
                "attempt": attempt,
                "input_tokens": usage.input,
                "output_tokens": usage.output,
                "cache_read": usage.cache_read,
                "cache_write": usage.cache_write,
                "error": error,
            });
            if let Some(m) = metrics {
                if let serde_json::Value::Object(ref mut map) = data {
                    map.insert("duration_ms".into(), json!(m.duration_ms));
                    map.insert("ttfb_ms".into(), json!(m.ttfb_ms));
                    map.insert("ttft_ms".into(), json!(m.ttft_ms));
                    map.insert("streaming_ms".into(), json!(m.streaming_ms));
                    map.insert("chunk_count".into(), json!(m.chunk_count));
                }
            }
            events.push(json!({
                "type": "llm_call_completed",
                "data": data,
            }));
        }
        RunEventPayload::ContextCompactionStarted {
            message_count,
            estimated_tokens,
            budget_tokens,
            system_prompt_tokens,
            context_window,
        } => {
            events.push(json!({
                "type": "context_compaction_started",
                "data": {
                    "message_count": message_count,
                    "estimated_tokens": estimated_tokens,
                    "budget_tokens": budget_tokens,
                    "system_prompt_tokens": system_prompt_tokens,
                    "context_window": context_window,
                }
            }));
        }
        RunEventPayload::ContextCompactionCompleted { result } => {
            events.push(json!({
                "type": "context_compaction_completed",
                "data": {
                    "result": result,
                }
            }));
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
