use axum::response::sse::Event as SseEvent;

use super::http::should_skip_event;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;

pub fn map_event_to_sse(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    event: &Event,
) -> Option<SseEvent> {
    if should_skip_event(event) {
        return None;
    }

    let (event_name, payload) = match event {
        Event::Start => (
            "RunStarted",
            base_event_payload(agent_id, session_id, run_id, "RunStarted"),
        ),
        Event::ReasonStart => (
            "ReasoningStarted",
            base_event_payload(agent_id, session_id, run_id, "ReasoningStarted"),
        ),
        Event::ReasonEnd { finish_reason } => {
            let mut payload =
                base_event_payload(agent_id, session_id, run_id, "ReasoningCompleted");
            payload["finish_reason"] = serde_json::Value::String(finish_reason.clone());
            ("ReasoningCompleted", payload)
        }
        Event::StreamDelta(Delta::Text { content }) => {
            let mut payload = base_event_payload(agent_id, session_id, run_id, "RunContent");
            payload["content"] = serde_json::Value::String(content.clone());
            payload["content_type"] = serde_json::Value::String("str".to_string());
            ("RunContent", payload)
        }
        Event::StreamDelta(Delta::Thinking { content }) => {
            let mut payload =
                base_event_payload(agent_id, session_id, run_id, "ReasoningContentDelta");
            payload["content"] = serde_json::Value::String(content.clone());
            ("ReasoningContentDelta", payload)
        }
        Event::StreamDelta(Delta::Done { .. }) => (
            "RunContentCompleted",
            base_event_payload(agent_id, session_id, run_id, "RunContentCompleted"),
        ),
        Event::ToolStart {
            tool_call_id,
            name,
            arguments,
        } => {
            let mut payload = base_event_payload(agent_id, session_id, run_id, "ToolCallStarted");
            payload["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
            payload["tool_name"] = serde_json::Value::String(name.clone());
            payload["arguments"] = arguments.clone();
            ("ToolCallStarted", payload)
        }
        Event::ToolEnd {
            tool_call_id,
            name,
            success,
            output,
            ..
        } => {
            let event_name = if *success {
                "ToolCallCompleted"
            } else {
                "ToolCallError"
            };
            let mut payload = base_event_payload(agent_id, session_id, run_id, event_name);
            payload["tool_call_id"] = serde_json::Value::String(tool_call_id.clone());
            payload["tool_name"] = serde_json::Value::String(name.clone());
            payload["content"] = serde_json::Value::String(output.clone());
            payload["success"] = serde_json::Value::Bool(*success);
            (event_name, payload)
        }
        Event::CompactionDone {
            messages_before,
            messages_after,
            summary_len,
        } => {
            let mut payload =
                base_event_payload(agent_id, session_id, run_id, "CompressionCompleted");
            payload["messages_before"] = serde_json::json!(messages_before);
            payload["messages_after"] = serde_json::json!(messages_after);
            payload["summary_len"] = serde_json::json!(summary_len);
            ("CompressionCompleted", payload)
        }
        Event::ReasonError { error } | Event::Error { message: error } => {
            let mut payload = base_event_payload(agent_id, session_id, run_id, "RunError");
            payload["content"] = serde_json::Value::String(error.clone());
            ("RunError", payload)
        }
        Event::End {
            stop_reason, usage, ..
        } => {
            let event_name = match stop_reason.as_str() {
                "end_turn" => "RunCompleted",
                "timeout" | "max_iterations" => "RunPaused",
                "aborted" => "RunCancelled",
                _ => "RunError",
            };
            let mut payload = base_event_payload(agent_id, session_id, run_id, event_name);
            payload["stop_reason"] = serde_json::Value::String(stop_reason.clone());
            payload["metrics"] = serde_json::json!({
                "prompt_tokens": usage.prompt_tokens,
                "completion_tokens": usage.completion_tokens,
                "reasoning_tokens": usage.reasoning_tokens,
                "total_tokens": usage.total_tokens,
                "cache_read_tokens": usage.cache_read_tokens,
                "cache_write_tokens": usage.cache_write_tokens,
                "ttft_ms": usage.ttft_ms,
            });
            (event_name, payload)
        }
        _ => return None,
    };

    Some(encode_sse(event_name, payload))
}

pub fn base_event_payload(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    event: &str,
) -> serde_json::Value {
    serde_json::json!({
        "created_at": crate::storage::time::now().timestamp(),
        "event": event,
        "agent_id": agent_id,
        "run_id": run_id,
        "session_id": session_id,
    })
}

pub fn encode_sse(event: &str, payload: serde_json::Value) -> SseEvent {
    SseEvent::default().event(event).data(payload.to_string())
}
