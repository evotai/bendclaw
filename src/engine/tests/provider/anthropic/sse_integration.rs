//! Integration tests: Anthropic provider → wiremock SSE server → Message.

use bendengine::provider::AnthropicProvider;
use bendengine::provider::StreamEvent;
use bendengine::types::*;

use super::super::helpers::provider_helper::*;

// ---------------------------------------------------------------------------
// SSE streaming — text response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_text_response() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(100, 0),
        anthropic_sse::text_block_start(0),
        anthropic_sse::text_delta(0, "Hello, "),
        anthropic_sse::text_delta(0, "world!"),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("end_turn", 10),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            usage,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello, world!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(usage.input, 100);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec!["Hello, ", "world!"]);
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}

// ---------------------------------------------------------------------------
// SSE streaming — tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_tool_call() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::tool_block_start(0, "toolu_123", "bash"),
        anthropic_sse::tool_input_delta(0, r#"{"command": "ls -la"}"#),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("tool_use", 5),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant {
            content,
            stop_reason,
            ..
        } => {
            assert_eq!(content.len(), 1);
            assert!(
                matches!(&content[0], Content::ToolCall { id, name, arguments }
                    if id == "toolu_123" && name == "bash" && arguments["command"] == "ls -la")
            );
            assert_eq!(*stop_reason, StopReason::ToolUse);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallStart { name, .. } if name == "bash")));
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ToolCallEnd { .. })));
}

// ---------------------------------------------------------------------------
// SSE streaming — thinking + text
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_thinking_then_text() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(80, 0),
        anthropic_sse::thinking_block_start(0),
        anthropic_sse::thinking_delta(0, "Let me think..."),
        anthropic_sse::block_stop(0),
        anthropic_sse::text_block_start(1),
        anthropic_sse::text_delta(1, "The answer is 42."),
        anthropic_sse::block_stop(1),
        anthropic_sse::message_delta("end_turn", 20),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { content, .. } => {
            assert_eq!(content.len(), 2);
            assert!(
                matches!(&content[0], Content::Thinking { thinking, .. } if thinking == "Let me think...")
            );
            assert!(matches!(&content[1], Content::Text { text } if text == "The answer is 42."));
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::ThinkingDelta { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, StreamEvent::TextDelta { .. })));
}

// ---------------------------------------------------------------------------
// SSE streaming — error event (overloaded)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_error_event() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(50, 0),
        anthropic_sse::error("overloaded_error", "Overloaded"),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap_err();

    assert!(bendengine::retry::should_retry(&err));
}

// ---------------------------------------------------------------------------
// SSE streaming — usage with cache
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_sse_cache_usage() {
    let sse = anthropic_sse::body(vec![
        anthropic_sse::message_start(100, 500),
        anthropic_sse::text_block_start(0),
        anthropic_sse::text_delta(0, "cached"),
        anthropic_sse::block_stop(0),
        anthropic_sse::message_delta("end_turn", 5),
        anthropic_sse::message_stop(),
    ]);

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, _) = run_provider_sse(&AnthropicProvider, config, &sse, 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { usage, .. } => {
            assert_eq!(usage.input, 100);
            assert_eq!(usage.cache_read, 500);
        }
        _ => panic!("Expected Assistant message"),
    }
}

// ---------------------------------------------------------------------------
// HTTP error — 429 rate limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_http_429_rate_limited() {
    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_json(
        &AnthropicProvider,
        config,
        r#"{"error":{"type":"rate_limit_error","message":"Rate limited"}}"#,
        429,
    )
    .await
    .unwrap_err();

    assert!(matches!(
        err,
        bendengine::provider::ProviderError::RateLimited { .. }
    ));
}

// ---------------------------------------------------------------------------
// HTTP error — 400 context overflow
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_http_400_context_overflow() {
    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let err = run_provider_json(
        &AnthropicProvider,
        config,
        r#"{"error":{"type":"invalid_request_error","message":"prompt is too long: 213462 tokens > 200000 maximum"}}"#,
        400,
    )
    .await
    .unwrap_err();

    assert!(err.is_context_overflow());
}

// ---------------------------------------------------------------------------
// JSON fallback — success response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn anthropic_json_fallback_success() {
    let json = serde_json::json!({
        "id": "msg_test",
        "type": "message",
        "role": "assistant",
        "content": [{"type": "text", "text": "Hello from JSON!"}],
        "stop_reason": "end_turn",
        "usage": {"input_tokens": 50, "output_tokens": 10}
    });

    let config = StreamConfigBuilder::anthropic().cache_disabled().build();
    let (msg, events) = run_provider_json(&AnthropicProvider, config, &json.to_string(), 200)
        .await
        .unwrap();

    match &msg {
        Message::Assistant { content, usage, .. } => {
            assert_eq!(content.len(), 1);
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from JSON!"));
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}
