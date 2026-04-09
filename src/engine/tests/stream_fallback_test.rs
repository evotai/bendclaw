//! Tests for the shared FallbackEmitter.

use bendengine::provider::stream_fallback::FallbackEmitter;
use bendengine::provider::StreamEvent;
use bendengine::types::*;

#[test]
fn fallback_emitter_text_only() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let mut emitter = FallbackEmitter::new(tx);
    emitter.emit_text("Hello world");
    emitter.set_stop_reason(StopReason::Stop);
    let msg = emitter.finalize("test-model", "test-provider");

    // Verify message
    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            model,
            provider,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello world"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(model, "test-model");
            assert_eq!(provider, "test-provider");
        }
        _ => panic!("Expected Assistant message"),
    }

    // Verify events
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(&events[1], StreamEvent::TextDelta { delta, .. } if delta == "Hello world"));
    assert!(matches!(&events[2], StreamEvent::Done { .. }));
}

#[test]
fn fallback_emitter_tool_call() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let mut emitter = FallbackEmitter::new(tx);
    emitter.emit_tool_call("tc-1", "bash", serde_json::json!({"command": "ls"}));
    emitter.set_stop_reason(StopReason::ToolUse);
    let msg = emitter.finalize("model", "provider");

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, .. } if id == "tc-1" && name == "bash")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(
        matches!(&events[1], StreamEvent::ToolCallStart { id, name, .. } if id == "tc-1" && name == "bash")
    );
    assert!(matches!(&events[2], StreamEvent::ToolCallEnd { .. }));
    assert!(matches!(&events[3], StreamEvent::Done { .. }));
}

#[test]
fn fallback_emitter_thinking() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let mut emitter = FallbackEmitter::new(tx);
    emitter.emit_thinking("Let me think...", Some("sig123".into()));
    emitter.emit_text("The answer is 42.");
    let msg = emitter.finalize("model", "provider");

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::Thinking { thinking, signature } if thinking == "Let me think..." && signature.as_deref() == Some("sig123"))
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

#[test]
fn fallback_emitter_empty_text_skipped() {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let mut emitter = FallbackEmitter::new(tx);
    emitter.emit_text("");
    emitter.emit_thinking("", None);
    let msg = emitter.finalize("model", "provider");

    match &msg {
        Message::Assistant { content, .. } => {
            assert!(content.is_empty());
        }
        _ => panic!("Expected Assistant message"),
    }

    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    // Only Start + Done, no text/thinking deltas
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0], StreamEvent::Start));
    assert!(matches!(events[1], StreamEvent::Done { .. }));
}

#[test]
fn fallback_emitter_usage() {
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<StreamEvent>();

    let mut emitter = FallbackEmitter::new(tx);
    emitter.emit_text("hi");
    emitter.set_usage(Usage {
        input: 100,
        output: 50,
        cache_read: 10,
        cache_write: 5,
        total_tokens: 165,
    });
    let msg = emitter.finalize("model", "provider");

    match &msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 50);
            assert_eq!(usage.cache_read, 10);
            assert_eq!(usage.cache_write, 5);
            assert_eq!(usage.total_tokens, 165);
        }
        _ => panic!("Expected Assistant message"),
    }
}
