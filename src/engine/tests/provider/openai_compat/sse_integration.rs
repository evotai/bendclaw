//! Integration tests: OpenAI-compat provider → wiremock SSE server → Message.

use bendengine::provider::traits::StreamConfig;
use bendengine::provider::OpenAiCompatProvider;
use bendengine::provider::StreamEvent;
use bendengine::types::*;

use super::super::helpers::provider_helper::*;

/// OpenAI config pointing at a mock server base_url.
fn openai_config() -> StreamConfig {
    StreamConfigBuilder::openai()
        .system_prompt("You are helpful.")
        .cache_disabled()
        .build()
}

// ---------------------------------------------------------------------------
// SSE streaming — text response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_text_response() {
    let sse = openai_sse::body(vec![
        openai_sse::text_chunk("Hello, ", None),
        openai_sse::text_chunk("world!", None),
        openai_sse::finish_with_usage("stop", 50, 10),
        openai_sse::done(),
    ]);

    let (msg, events) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
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
            assert_eq!(usage.input, 50);
            assert_eq!(usage.output, 10);
        }
        _ => panic!("Expected Assistant message"),
    }

    let text_deltas: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            StreamEvent::TextDelta { delta, .. } => Some(delta.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text_deltas, vec!["Hello, ", "world!"]);
}

// ---------------------------------------------------------------------------
// SSE streaming — tool call
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_tool_call() {
    let sse = openai_sse::body(vec![
        openai_sse::tool_call_start(0, "call_abc", "bash"),
        openai_sse::tool_call_args(0, r#"{"command": "ls"}"#),
        openai_sse::finish_with_usage("tool_calls", 40, 8),
        openai_sse::done(),
    ]);

    let (msg, events) = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
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
                    if id == "call_abc" && name == "bash" && arguments["command"] == "ls")
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
// SSE streaming — empty response is error
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_empty_response_is_error() {
    let sse = openai_sse::body(vec![openai_sse::done()]);

    let err = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap_err();

    assert!(matches!(err, bendengine::provider::ProviderError::Api(_)));
}

// ---------------------------------------------------------------------------
// SSE streaming — inline error chunk
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_sse_inline_error() {
    let sse = openai_sse::body(vec![
        format!(
            "data: {}",
            serde_json::json!({
                "choices": [],
                "error": {"message": "upstream failed"}
            })
        ),
        openai_sse::done(),
    ]);

    let err = run_provider_sse(&OpenAiCompatProvider, openai_config(), &sse, 200)
        .await
        .unwrap_err();

    assert!(matches!(
        err,
        bendengine::provider::ProviderError::Api(ref msg) if msg.contains("upstream failed")
    ));
}

// ---------------------------------------------------------------------------
// HTTP error — 429 rate limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_http_429_rate_limited() {
    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        r#"{"error":{"message":"Rate limited","type":"rate_limit_error"}}"#,
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
async fn openai_http_400_context_overflow() {
    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        r#"{"error":{"message":"Your input exceeds the context window of this model","type":"invalid_request_error"}}"#,
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
async fn openai_json_fallback_success() {
    let json = serde_json::json!({
        "id": "chatcmpl-test",
        "object": "chat.completion",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": "Hello from JSON!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 30,
            "completion_tokens": 5,
            "total_tokens": 35
        }
    });

    let (msg, events) = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        &json.to_string(),
        200,
    )
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
            assert!(matches!(&content[0], Content::Text { text } if text == "Hello from JSON!"));
            assert_eq!(*stop_reason, StopReason::Stop);
            assert_eq!(usage.input, 30);
            assert_eq!(usage.output, 5);
        }
        _ => panic!("Expected Assistant message"),
    }

    assert!(events.iter().any(|e| matches!(e, StreamEvent::Start)));
    assert!(events.iter().any(|e| matches!(e, StreamEvent::Done { .. })));
}

// ---------------------------------------------------------------------------
// JSON fallback — error response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_json_fallback_error() {
    let json = serde_json::json!({
        "error": {
            "message": "Internal server error",
            "type": "server_error"
        }
    });

    let err = run_provider_json(
        &OpenAiCompatProvider,
        openai_config(),
        &json.to_string(),
        200,
    )
    .await
    .unwrap_err();

    assert!(matches!(err, bendengine::provider::ProviderError::Api(_)));
}
