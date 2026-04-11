//! Tests for OpenAI-compatible JSON fallback handling.

use bendengine::provider::stream_http::classify_json_error;
use bendengine::provider::ProviderError;

// ---------------------------------------------------------------------------
// Error-shaped JSON classification
// ---------------------------------------------------------------------------

#[test]
fn openai_error_generic() {
    let value = serde_json::json!({
        "error": {
            "message": "server error",
            "type": "server_error"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(bendengine::retry::should_retry(&err));
}

#[test]
fn openai_error_context_overflow() {
    let value = serde_json::json!({
        "error": {
            "message": "Your input exceeds the context window of this model",
            "type": "invalid_request_error"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!bendengine::retry::should_retry(&err));
}

// ---------------------------------------------------------------------------
// Success-shaped JSON → FallbackEmitter (integration-style)
// ---------------------------------------------------------------------------

#[test]
fn openai_success_text_response() {
    use bendengine::provider::stream_fallback::FallbackEmitter;
    use bendengine::provider::StreamEvent;
    use bendengine::types::*;

    let value: serde_json::Value = serde_json::json!({
        "id": "chatcmpl-123",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello from OpenAI!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 50,
            "completion_tokens": 10,
            "total_tokens": 60
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    // Simulate what json_fallback::parse_success_response does
    if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(msg) = choice.get("message") {
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    emitter.emit_text(text);
                }
            }
            let stop_reason = match choice.get("finish_reason").and_then(|f| f.as_str()) {
                Some("stop") => StopReason::Stop,
                Some("length") => StopReason::Length,
                Some("tool_calls") => StopReason::ToolUse,
                _ => StopReason::Stop,
            };
            emitter.set_stop_reason(stop_reason);
        }
    }

    if let Some(u) = value.get("usage") {
        let usage = Usage {
            input: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output: u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            ..Default::default()
        };
        emitter.set_usage(usage);
    }

    let msg = emitter.finalize("gpt-4o", "openai");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            model,
            provider,
            usage,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from OpenAI!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(model, "gpt-4o");
            assert_eq!(provider, "openai");
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
            assert_eq!(usage.total_tokens, 60);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(
        matches!(&events[1], StreamEvent::TextDelta { delta, .. } if delta == "Hello from OpenAI!")
    );
    assert!(matches!(&events[2], StreamEvent::Done { .. }));
}

#[test]
fn openai_success_tool_calls_response() {
    use bendengine::provider::stream_fallback::FallbackEmitter;
    use bendengine::provider::StreamEvent;
    use bendengine::types::*;

    let value: serde_json::Value = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "bash",
                        "arguments": "{\"command\":\"ls -la\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 80,
            "completion_tokens": 15,
            "total_tokens": 95
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(msg) = choice.get("message") {
                if let Some(tool_calls) = msg.get("tool_calls").and_then(|t| t.as_array()) {
                    for tc in tool_calls {
                        let id = tc
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("")
                            .to_string();
                        let args_str = tc
                            .get("function")
                            .and_then(|f| f.get("arguments"))
                            .and_then(|a| a.as_str())
                            .unwrap_or("{}");
                        let arguments: serde_json::Value =
                            serde_json::from_str(args_str).unwrap_or(serde_json::json!({}));
                        emitter.emit_tool_call(&id, &name, arguments);
                    }
                }
            }
            emitter.set_stop_reason(StopReason::ToolUse);
        }
    }

    let msg = emitter.finalize("gpt-4o", "openai");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, arguments } if id == "call_abc123" && name == "bash" && arguments["command"] == "ls -la")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(
        matches!(&events[1], StreamEvent::ToolCallStart { id, name, .. } if id == "call_abc123" && name == "bash")
    );
    assert!(matches!(&events[2], StreamEvent::ToolCallEnd { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}

#[test]
fn openai_success_with_reasoning() {
    use bendengine::provider::stream_fallback::FallbackEmitter;
    use bendengine::provider::StreamEvent;
    use bendengine::types::*;

    let value: serde_json::Value = serde_json::json!({
        "choices": [{
            "message": {
                "role": "assistant",
                "reasoning_content": "Let me think about this...",
                "content": "The answer is 42."
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 30,
            "completion_tokens": 20,
            "total_tokens": 50
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    if let Some(choices) = value.get("choices").and_then(|c| c.as_array()) {
        if let Some(choice) = choices.first() {
            if let Some(msg) = choice.get("message") {
                // Reasoning
                if let Some(reasoning) = msg
                    .get("reasoning_content")
                    .and_then(|r| r.as_str())
                    .or_else(|| msg.get("reasoning").and_then(|r| r.as_str()))
                {
                    emitter.emit_thinking(reasoning, None);
                }
                // Text
                if let Some(text) = msg.get("content").and_then(|c| c.as_str()) {
                    emitter.emit_text(text);
                }
            }
        }
    }

    let msg = emitter.finalize("deepseek-r1", "deepseek");

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::Thinking { thinking, .. } if thinking == "Let me think about this...")
            );
            assert!(matches!(&content[1], Content::Text { text } if text == "The answer is 42."));
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(&events[1], StreamEvent::ThinkingDelta { .. }));
    assert!(matches!(&events[2], StreamEvent::TextDelta { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}
