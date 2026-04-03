use std::time::Duration;
use std::time::Instant;

use bendclaw::execution::llm::assistant_turn::aborted_tool_result_messages;
use bendclaw::execution::llm::assistant_turn::assistant_message_from_turn;
use bendclaw::execution::llm::assistant_turn::record_assistant_turn;
use bendclaw::execution::llm::engine_state::RunLoopConfig;
use bendclaw::execution::llm::engine_state::RunLoopState;
use bendclaw::execution::llm::response_mapper::LLMResponse;
use bendclaw::llm::message::ToolCall;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::usage::TokenUsage;
use bendclaw::sessions::Message;

fn tool_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ContentDelta("use tool".to_string()));
    turn.apply_stream_event(StreamEvent::ToolCallEnd {
        index: 0,
        id: "tc-1".to_string(),
        name: "shell".to_string(),
        arguments: r#"{"command":"ls"}"#.to_string(),
    });
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(10, 5)));
    turn.set_ttft_ms(42);
    turn
}

fn final_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ThinkingDelta("reason".to_string()));
    turn.apply_stream_event(StreamEvent::ContentDelta("final answer".to_string()));
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(7, 3)));
    turn.set_ttft_ms(15);
    turn
}

fn loop_state() -> RunLoopState {
    RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(30),
            max_context_tokens: 8192,
        },
        Instant::now(),
    )
}

#[test]
fn assistant_message_from_turn_keeps_tool_calls_and_metrics() {
    let turn = tool_turn();
    let message =
        assistant_message_from_turn(&turn, "mock-model", Duration::from_secs(60), "run-1");

    match message {
        Message::Assistant {
            content,
            tool_calls,
            origin_run_id,
            operation,
            metrics,
        } => {
            assert_eq!(content, "use tool");
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "shell");
            assert_eq!(origin_run_id.as_deref(), Some("run-1"));
            assert_eq!(operation.summary, "mock-model -> 15 tokens");
            let metrics = metrics.expect("assistant metrics");
            assert_eq!(metrics.input_tokens, 10);
            assert_eq!(metrics.output_tokens, 5);
            assert_eq!(metrics.ttft_ms, 42);
        }
        other => panic!("expected assistant message, got {other:?}"),
    }
}

#[test]
fn record_assistant_turn_without_tool_calls_sets_final_content() {
    let turn = final_turn();
    let mut messages = Vec::new();
    let mut state = loop_state();

    record_assistant_turn(
        &mut messages,
        &turn,
        &mut state,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].origin_run_id(), Some("run-1"));
    assert!(!state.should_continue());
    assert_eq!(state.final_content().len(), 2);
    match &state.final_content()[1] {
        bendclaw::execution::ContentBlock::Text { text } => assert_eq!(text, "final answer"),
        other => panic!("expected final text block, got {other:?}"),
    }
}

#[test]
fn record_assistant_turn_with_tool_calls_keeps_loop_open() {
    let turn = tool_turn();
    let mut messages = Vec::new();
    let mut state = loop_state();

    record_assistant_turn(
        &mut messages,
        &turn,
        &mut state,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].origin_run_id(), Some("run-1"));
    assert!(state.should_continue());
    assert!(state.final_content().is_empty());
}

#[test]
fn aborted_tool_result_messages_returns_one_failed_tool_result_per_call() {
    let tool_calls = vec![
        ToolCall {
            id: "tc-1".to_string(),
            name: "shell".to_string(),
            arguments: "{}".to_string(),
        },
        ToolCall {
            id: "tc-2".to_string(),
            name: "memory_write".to_string(),
            arguments: "{}".to_string(),
        },
    ];

    let messages = aborted_tool_result_messages(&tool_calls, "run-1");

    assert_eq!(messages.len(), 2);
    for (message, tool_call) in messages.iter().zip(tool_calls.iter()) {
        match message {
            Message::ToolResult {
                tool_call_id,
                name,
                output,
                success,
                ..
            } => {
                assert_eq!(tool_call_id, &tool_call.id);
                assert_eq!(name, &tool_call.name);
                assert_eq!(output, "aborted");
                assert!(!success);
                assert_eq!(message.origin_run_id(), Some("run-1"));
            }
            other => panic!("expected tool result, got {other:?}"),
        }
    }
}
