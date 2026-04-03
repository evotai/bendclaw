use std::time::Duration;
use std::time::Instant;

use bendclaw::kernel::run::execution::llm::engine_state::RunLoopConfig;
use bendclaw::kernel::run::execution::llm::engine_state::RunLoopState;
use bendclaw::kernel::run::execution::llm::response_mapper::LLMResponse;
use bendclaw::kernel::run::execution::llm::transition::apply_turn_result;
use bendclaw::kernel::run::execution::llm::transition::TurnTransition;
use bendclaw::kernel::run::Reason;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::usage::TokenUsage;
use bendclaw::sessions::Message;

fn run_loop_state() -> RunLoopState {
    RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(60),
            max_context_tokens: 8192,
        },
        Instant::now(),
    )
}

fn final_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ThinkingDelta("reason".to_string()));
    turn.apply_stream_event(StreamEvent::ContentDelta("final answer".to_string()));
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(7, 3)));
    turn.set_ttft_ms(15);
    turn
}

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

#[test]
fn apply_turn_result_records_error_and_returns_error_transition() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = final_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        Some("boom"),
        None,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(transition, TurnTransition::Error(Reason::Error));
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        &messages[0],
        Message::OperationEvent { kind, name, status, .. }
            if kind == "llm" && name == "reasoning.turn" && status == "failed"
    ));
    assert!(matches!(
        &messages[1],
        Message::Error { message, .. } if message == "boom"
    ));
    assert!(!state.should_continue());
}

#[test]
fn apply_turn_result_returns_done_and_records_final_content() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = final_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(transition, TurnTransition::Done);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].origin_run_id(), Some("run-1"));
    assert!(!state.should_continue());
    assert_eq!(state.final_content().len(), 2);
}

#[test]
fn apply_turn_result_returns_dispatch_for_tool_turn() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = tool_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(transition, TurnTransition::DispatchTools);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].origin_run_id(), Some("run-1"));
    assert!(state.should_continue());
    assert!(matches!(
        &messages[0],
        Message::Assistant { tool_calls, .. } if tool_calls.len() == 1
    ));
}

#[test]
fn apply_turn_result_returns_abort_and_appends_aborted_tool_results() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = tool_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        Some(Reason::Timeout),
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(transition, TurnTransition::Abort(Reason::Timeout));
    assert_eq!(messages.len(), 2);
    assert!(matches!(
        &messages[1],
        Message::ToolResult { output, success, .. } if output == "aborted" && !success
    ));
    assert_eq!(messages[1].origin_run_id(), Some("run-1"));
    assert!(state.should_continue());
}

fn max_tokens_turn() -> LLMResponse {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ContentDelta("partial output".to_string()));
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(10, 100)));
    turn.apply_stream_event(StreamEvent::Done {
        finish_reason: "max_tokens".to_string(),
        provider: None,
        model: None,
    });
    turn
}

#[test]
fn max_tokens_triggers_continue_with_continuation_message() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();
    let turn = max_tokens_turn();

    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );

    assert_eq!(transition, TurnTransition::Continue);
    assert!(state.should_continue());
    // assistant message + user continuation prompt
    assert_eq!(messages.len(), 2);
    assert!(matches!(&messages[1], Message::User { .. }));
    assert_eq!(messages[0].origin_run_id(), Some("run-1"));
    assert_eq!(messages[1].origin_run_id(), Some("run-1"));
}

#[test]
fn max_tokens_fifth_consecutive_triggers_done() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();

    for i in 0..5 {
        messages.clear();
        let turn = max_tokens_turn();
        let transition = apply_turn_result(
            &mut messages,
            &mut state,
            &turn,
            None,
            None,
            "mock-model",
            Duration::from_secs(60),
            "run-1",
        );
        if i < 4 {
            assert_eq!(transition, TurnTransition::Continue, "iteration {i}");
        } else {
            assert_eq!(transition, TurnTransition::Done, "iteration {i}");
        }
    }
}

#[test]
fn non_max_tokens_turn_resets_streak() {
    let mut messages = Vec::new();
    let mut state = run_loop_state();

    // Two max_tokens continuations
    for _ in 0..2 {
        let turn = max_tokens_turn();
        apply_turn_result(
            &mut messages,
            &mut state,
            &turn,
            None,
            None,
            "mock-model",
            Duration::from_secs(60),
            "run-1",
        );
    }

    // Normal tool turn resets streak
    let turn = tool_turn();
    let transition = apply_turn_result(
        &mut messages,
        &mut state,
        &turn,
        None,
        None,
        "mock-model",
        Duration::from_secs(60),
        "run-1",
    );
    assert_eq!(transition, TurnTransition::DispatchTools);

    // Now 5 more max_tokens should all continue (streak was reset)
    for i in 0..5 {
        messages.clear();
        let turn = max_tokens_turn();
        let transition = apply_turn_result(
            &mut messages,
            &mut state,
            &turn,
            None,
            None,
            "mock-model",
            Duration::from_secs(60),
            "run-1",
        );
        if i < 4 {
            assert_eq!(transition, TurnTransition::Continue, "iteration {i}");
        } else {
            assert_eq!(transition, TurnTransition::Done, "iteration {i}");
        }
    }
}
