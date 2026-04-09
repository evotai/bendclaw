//! Tests for Anthropic JSON fallback handling.

use bendengine::provider::stream_http::classify_json_error;
use bendengine::provider::stream_http::extract_json_error_message;
use bendengine::provider::ProviderError;

// ---------------------------------------------------------------------------
// Error-shaped JSON classification
// ---------------------------------------------------------------------------

#[test]
fn anthropic_error_overloaded() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "overloaded_error",
            "message": "Overloaded"
        }
    });
    let err = classify_json_error(&value);
    assert!(matches!(err, ProviderError::Api(_)));
    assert!(err.is_retryable());
}

#[test]
fn anthropic_error_context_overflow() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "invalid_request_error",
            "message": "prompt is too long: 213462 tokens > 200000 maximum"
        }
    });
    let err = classify_json_error(&value);
    assert!(err.is_context_overflow());
    assert!(!err.is_retryable());
}

#[test]
fn anthropic_error_message_extraction() {
    let value = serde_json::json!({
        "type": "error",
        "error": {
            "type": "api_error",
            "message": "Internal server error"
        }
    });
    let msg = extract_json_error_message(&value);
    assert_eq!(msg, Some("api_error: Internal server error".into()));
}

// ---------------------------------------------------------------------------
// Success-shaped JSON → FallbackEmitter (integration-style)
// ---------------------------------------------------------------------------

#[test]
fn anthropic_success_text_response() {
    use bendengine::provider::stream_fallback::FallbackEmitter;
    use bendengine::provider::StreamEvent;
    use bendengine::types::*;

    let value = serde_json::json!({
        "id": "msg_123",
        "type": "message",
        "role": "assistant",
        "content": [
            {"type": "text", "text": "Hello, world!"}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 20,
            "cache_read_input_tokens": 50,
            "cache_creation_input_tokens": 10
        }
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    // Simulate what json_fallback::parse_success_response does
    if let Some(blocks) = value.get("content").and_then(|c| c.as_array()) {
        for block in blocks {
            if let Some("text") = block.get("type").and_then(|t| t.as_str()) {
                if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                    emitter.emit_text(text);
                }
            }
        }
    }

    if let Some(u) = value.get("usage") {
        let usage = Usage {
            input: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            output: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            cache_read: u
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_write: u
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            ..Default::default()
        };
        emitter.set_usage(usage);
    }

    let msg = emitter.finalize("claude-sonnet-4-20250514", "anthropic");

    match &msg {
        Message::Assistant {
            content,
            model,
            provider,
            usage,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello, world!"));
            assert_eq!(model, "claude-sonnet-4-20250514");
            assert_eq!(provider, "anthropic");
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 20);
            assert_eq!(usage.cache_read, 50);
            assert_eq!(usage.cache_write, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(&events[1], StreamEvent::TextDelta { delta, .. } if delta == "Hello, world!"));
    assert!(matches!(&events[2], StreamEvent::Done { .. }));
}

#[test]
fn anthropic_success_tool_use_response() {
    use bendengine::provider::stream_fallback::FallbackEmitter;
    use bendengine::provider::StreamEvent;
    use bendengine::types::*;

    let value = serde_json::json!({
        "content": [
            {
                "type": "tool_use",
                "id": "toolu_123",
                "name": "bash",
                "input": {"command": "ls -la"}
            }
        ],
        "stop_reason": "tool_use",
        "usage": {"input_tokens": 50, "output_tokens": 10}
    });

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();
    let mut emitter = FallbackEmitter::new(tx);

    if let Some(blocks) = value.get("content").and_then(|c| c.as_array()) {
        for block in blocks {
            if block.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                let id = block
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let name = block
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let arguments = block.get("input").cloned().unwrap_or(serde_json::json!({}));
                emitter.emit_tool_call(&id, &name, arguments);
            }
        }
    }

    emitter.set_stop_reason(StopReason::ToolUse);
    let msg = emitter.finalize("claude-sonnet-4-20250514", "anthropic");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, .. } if id == "toolu_123" && name == "bash")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(&events[1], StreamEvent::ToolCallStart { .. }));
    assert!(matches!(&events[2], StreamEvent::ToolCallEnd { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}
