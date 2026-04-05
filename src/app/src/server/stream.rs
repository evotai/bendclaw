use std::convert::Infallible;

use serde_json::json;

pub type SseEvent = std::result::Result<axum::response::sse::Event, Infallible>;

pub fn done_event() -> SseEvent {
    event("done", &json!(null))
}

pub fn error_event(message: impl Into<String>) -> SseEvent {
    event("error", &json!({ "message": message.into() }))
}

pub fn map_sdk_message(
    message: &bend_agent::SDKMessage,
    started_at: &std::time::Instant,
) -> Vec<SseEvent> {
    let mut events = Vec::new();

    match message {
        bend_agent::SDKMessage::Assistant { message, .. } => {
            for block in &message.content {
                match block {
                    bend_agent::ContentBlock::Text { text } if !text.is_empty() => {
                        events.push(event("text", &json!({ "text": text })));
                    }
                    bend_agent::ContentBlock::ToolUse { id, name, input } => {
                        events.push(event(
                            "tool_use",
                            &json!({ "id": id, "name": name, "input": input }),
                        ));
                    }
                    bend_agent::ContentBlock::Thinking { thinking, .. } if !thinking.is_empty() => {
                        events.push(event("thinking", &json!({ "thinking": thinking })));
                    }
                    _ => {}
                }
            }
        }
        bend_agent::SDKMessage::ToolResult {
            tool_use_id,
            content,
            is_error,
            ..
        } => {
            events.push(event(
                "tool_result",
                &json!({
                    "tool_use_id": tool_use_id,
                    "content": content,
                    "is_error": is_error,
                }),
            ));
        }
        bend_agent::SDKMessage::Result {
            usage,
            num_turns,
            cost_usd,
            ..
        } => {
            events.push(event(
                "result",
                &json!({
                    "num_turns": num_turns,
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cost": cost_usd,
                    "duration_ms": started_at.elapsed().as_millis() as u64,
                }),
            ));
        }
        bend_agent::SDKMessage::Error { message } => {
            events.push(error_event(message));
        }
        bend_agent::SDKMessage::PartialMessage { text } => {
            events.push(event("text", &json!({ "text": text })));
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
