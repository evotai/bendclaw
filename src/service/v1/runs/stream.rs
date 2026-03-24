use axum::response::sse::Event as SseEvent;

use super::service::should_skip_event;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::kernel::tools::cli_agent::AgentEvent;

pub fn map_event_to_sse(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    event: &Event,
) -> Option<SseEvent> {
    // ToolUpdate carries structured AgentEvent — map each kind to its own SSE event.
    if let Event::ToolUpdate {
        tool_call_id,
        event: agent_event,
    } = event
    {
        return Some(map_agent_event_to_sse(
            agent_id,
            session_id,
            run_id,
            tool_call_id,
            agent_event,
        ));
    }

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
            operation,
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
            payload["op_type"] = serde_json::Value::String(operation.op_type.to_string());
            payload["duration_ms"] = serde_json::json!(operation.duration_ms);
            payload["summary"] = serde_json::Value::String(operation.summary.clone());
            if let Some(ref impact) = operation.impact {
                payload["impact"] = serde_json::Value::String(impact.to_string());
            }
            (event_name, payload)
        }
        Event::Progress {
            tool_call_id,
            message,
        } => {
            let mut payload = base_event_payload(agent_id, session_id, run_id, "Progress");
            if let Some(ref id) = tool_call_id {
                payload["tool_call_id"] = serde_json::Value::String(id.clone());
            }
            payload["message"] = serde_json::Value::String(message.clone());
            ("Progress", payload)
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
        Event::Audit {
            name,
            payload: audit_payload,
        } => match name.as_str() {
            "llm.request" => {
                let mut payload =
                    base_event_payload(agent_id, session_id, run_id, "ModelRequestStarted");
                if let Some(model) = audit_payload.get("model") {
                    payload["model"] = model.clone();
                }
                if let Some(temperature) = audit_payload.get("temperature") {
                    payload["temperature"] = temperature.clone();
                }
                if let Some(iteration) = audit_payload.get("iteration") {
                    payload["iteration"] = iteration.clone();
                }
                ("ModelRequestStarted", payload)
            }
            "llm.response" => {
                let mut payload =
                    base_event_payload(agent_id, session_id, run_id, "ModelRequestCompleted");
                if let Some(model) = audit_payload.get("model") {
                    payload["model"] = model.clone();
                }
                if let Some(provider) = audit_payload.get("provider") {
                    payload["provider"] = provider.clone();
                }
                if let Some(finish_reason) = audit_payload.get("finish_reason") {
                    payload["finish_reason"] = finish_reason.clone();
                }
                if let Some(usage) = audit_payload.get("usage") {
                    if let Some(obj) = usage.as_object() {
                        payload["input_tokens"] = serde_json::json!(obj
                            .get("prompt_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0));
                        payload["output_tokens"] = serde_json::json!(obj
                            .get("completion_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0));
                        payload["total_tokens"] = serde_json::json!(obj
                            .get("total_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0));
                        payload["reasoning_tokens"] = serde_json::json!(obj
                            .get("reasoning_tokens")
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0));
                    }
                }
                if let Some(ttft) = audit_payload.get("ttft_ms") {
                    payload["time_to_first_token"] = ttft.clone();
                }
                ("ModelRequestCompleted", payload)
            }
            "llm.error" => {
                let mut payload =
                    base_event_payload(agent_id, session_id, run_id, "ModelRequestCompleted");
                if let Some(model) = audit_payload.get("model") {
                    payload["model"] = model.clone();
                }
                if let Some(error) = audit_payload.get("error") {
                    payload["error"] = error.clone();
                }
                payload["status"] = serde_json::json!("error");
                ("ModelRequestCompleted", payload)
            }
            "turn.started" => {
                let mut payload = base_event_payload(agent_id, session_id, run_id, "TurnStarted");
                if let Some(iteration) = audit_payload.get("iteration") {
                    payload["iteration"] = iteration.clone();
                }
                ("TurnStarted", payload)
            }
            "turn.completed" => {
                let mut payload = base_event_payload(agent_id, session_id, run_id, "TurnCompleted");
                if let Some(iteration) = audit_payload.get("iteration") {
                    payload["iteration"] = iteration.clone();
                }
                if let Some(outcome) = audit_payload.get("outcome") {
                    payload["outcome"] = outcome.clone();
                }
                ("TurnCompleted", payload)
            }
            _ => return None,
        },
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
        "created_at": crate::storage::time::now().timestamp_millis(),
        "event": event,
        "agent_id": agent_id,
        "run_id": run_id,
        "session_id": session_id,
    })
}

pub fn encode_sse(event: &str, payload: serde_json::Value) -> SseEvent {
    SseEvent::default().event(event).data(payload.to_string())
}

/// Maps an AgentEvent to (sse_event_name, payload). Exposed for testing.
pub fn map_agent_event(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    tool_call_id: &str,
    agent_event: &AgentEvent,
) -> (String, serde_json::Value) {
    let (event_name, mut payload) = match agent_event {
        AgentEvent::Text { content } => {
            let mut p = base_event_payload(agent_id, session_id, run_id, "ToolCallUpdate");
            p["content"] = serde_json::Value::String(content.clone());
            ("ToolCallUpdate", p)
        }
        AgentEvent::Thinking { content } => {
            let mut p = base_event_payload(agent_id, session_id, run_id, "ToolCallThinking");
            p["content"] = serde_json::Value::String(content.clone());
            ("ToolCallThinking", p)
        }
        AgentEvent::ToolUse {
            tool_name,
            tool_use_id,
            input,
        } => {
            let mut p = base_event_payload(agent_id, session_id, run_id, "ToolCallSubToolStarted");
            p["tool_name"] = serde_json::Value::String(tool_name.clone());
            p["sub_tool_call_id"] = serde_json::Value::String(tool_use_id.clone());
            p["arguments"] = input.clone();
            ("ToolCallSubToolStarted", p)
        }
        AgentEvent::ToolResult {
            tool_use_id,
            success,
            output,
        } => {
            let mut p =
                base_event_payload(agent_id, session_id, run_id, "ToolCallSubToolCompleted");
            p["sub_tool_call_id"] = serde_json::Value::String(tool_use_id.clone());
            p["success"] = serde_json::Value::Bool(*success);
            p["content"] = serde_json::Value::String(output.clone());
            ("ToolCallSubToolCompleted", p)
        }
        AgentEvent::System { subtype, metadata } => {
            let mut p = base_event_payload(agent_id, session_id, run_id, "ToolCallStatus");
            p["subtype"] = serde_json::Value::String(subtype.clone());
            p["metadata"] = metadata.clone();
            ("ToolCallStatus", p)
        }
        AgentEvent::Error { message } => {
            let mut p = base_event_payload(agent_id, session_id, run_id, "ToolCallError");
            p["content"] = serde_json::Value::String(message.clone());
            ("ToolCallError", p)
        }
    };
    payload["tool_call_id"] = serde_json::Value::String(tool_call_id.to_string());
    payload["agent_event_kind"] = serde_json::Value::String(agent_event.kind().to_string());
    (event_name.to_string(), payload)
}

fn map_agent_event_to_sse(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    tool_call_id: &str,
    agent_event: &AgentEvent,
) -> SseEvent {
    let (name, payload) = map_agent_event(agent_id, session_id, run_id, tool_call_id, agent_event);
    encode_sse(&name, payload)
}
