use std::time::Duration;
use std::time::Instant;

use bendclaw::kernel::run::engine::abort::AbortPolicy;
use bendclaw::kernel::run::engine::abort::AbortSignal;
use bendclaw::kernel::run::engine::engine_state::RunLoopConfig;
use bendclaw::kernel::run::engine::engine_state::RunLoopState;
use bendclaw::kernel::run::engine::response_mapper::LLMResponse;
use bendclaw::kernel::run::ContentBlock;
use bendclaw::kernel::run::Reason;
use bendclaw::llm::stream::StreamEvent;
use bendclaw::llm::usage::TokenUsage;

// ── LLMResponse ──

#[test]
fn reasoning_turn_applies_stream_events_and_builds_preview() {
    let mut turn = LLMResponse::new();

    turn.apply_stream_event(StreamEvent::ThinkingDelta("t1".to_string()));
    turn.apply_stream_event(StreamEvent::ContentDelta("hello".to_string()));
    turn.apply_stream_event(StreamEvent::ToolCallEnd {
        index: 0,
        id: "tc1".to_string(),
        name: "tool_a".to_string(),
        arguments: "{}".to_string(),
    });
    turn.apply_stream_event(StreamEvent::Done {
        finish_reason: "stop".to_string(),
        provider: None,
        model: None,
    });

    assert!(turn.has_tool_calls());
    assert_eq!(turn.text(), "hello");
    assert_eq!(turn.finish_reason(), "stop");
    assert_eq!(turn.tool_calls().len(), 1);

    let preview = turn.response_preview();
    assert!(preview.contains("hello"));
    assert!(preview.contains("[tool_call] tool_a({})"));
}

#[test]
fn reasoning_turn_cancel_sets_error_state() {
    let mut turn = LLMResponse::new();
    assert!(!turn.has_error());

    turn.mark_cancelled();

    assert!(turn.has_error());
    let err = turn.take_error();
    assert_eq!(err.as_deref(), Some("cancelled"));
    assert!(!turn.has_error());
}

#[test]
fn reasoning_turn_exposes_debug_fingerprints() {
    let mut turn = LLMResponse::new();

    turn.set_debug_fingerprints(
        "content:1,done:1",
        "text>done",
        "text-hash",
        "thinking-hash",
        "tool-hash",
        "response-hash",
    );

    assert_eq!(turn.stream_event_summary(), "content:1,done:1");
    assert_eq!(turn.stream_event_sequence(), "text>done");
    assert_eq!(turn.text_fingerprint(), "text-hash");
    assert_eq!(turn.thinking_fingerprint(), "thinking-hash");
    assert_eq!(turn.tool_call_fingerprint(), "tool-hash");
    assert_eq!(turn.response_fingerprint(), "response-hash");
}
// ── RunLoopState ──

#[test]
fn turn_loop_state_tracks_iterations_and_usage() {
    let start = Instant::now();
    let mut state = RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(9),
            max_context_tokens: 2048,
        },
        start,
    );

    assert!(state.should_continue());
    assert_eq!(state.iterations(), 0);
    assert_eq!(state.max_context_tokens(), 2048);
    assert_eq!(state.deadline(), start + Duration::from_secs(9));

    assert_eq!(state.begin_iteration(), 1);
    assert_eq!(state.begin_iteration(), 2);

    state.add_token_usage(&TokenUsage::new(3, 2));
    assert_eq!(state.usage().prompt_tokens, 3);
    assert_eq!(state.usage().completion_tokens, 2);
    assert_eq!(state.usage().total_tokens, 5);
}

#[test]
fn turn_loop_state_stops_when_recording_final_content() {
    let mut state = RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(5),
            max_context_tokens: 1024,
        },
        Instant::now(),
    );
    state.begin_iteration();

    state.record_final_response(vec![ContentBlock::text("done")]);

    assert!(!state.should_continue());
    assert_eq!(state.final_content().len(), 1);

    let (content, iterations, usage) = state.into_finish();
    assert_eq!(iterations, 1);
    assert_eq!(content.len(), 1);
    assert_eq!(usage.total_tokens, 0);
}

#[test]
fn turn_loop_state_record_error_sets_final_content_and_stops() {
    let mut state = RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(5),
            max_context_tokens: 4096,
        },
        Instant::now(),
    );

    state.record_error("boom");

    assert!(!state.should_continue());
    let (content, _, _) = state.into_finish();
    let text = match &content[0] {
        ContentBlock::Text { text } => text.clone(),
        _ => String::new(),
    };
    assert_eq!(text, "LLM error: boom");
}
// ── AbortPolicy ──

#[test]
fn turn_loop_state_delegates_abort_decisions_to_policy() {
    let start = Instant::now();
    let mut state = RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(3),
            max_context_tokens: 1024,
        },
        start,
    );
    state.begin_iteration();

    let policy = AbortPolicy::new(1);
    let max = state.check_abort(&policy, false, start + Duration::from_secs(1));
    assert_eq!(max.signal, AbortSignal::MaxIterations);
    assert_eq!(max.reason, Some(Reason::MaxIterations));

    let timeout = state.check_cancel_or_timeout(&policy, false, start + Duration::from_secs(4));
    assert_eq!(timeout.signal, AbortSignal::Timeout);
    assert_eq!(timeout.reason, Some(Reason::Timeout));
}

#[test]
fn turn_loop_state_abort_decision_carries_aborted_reason() {
    let start = Instant::now();
    let state = RunLoopState::new(
        RunLoopConfig {
            max_duration: Duration::from_secs(5),
            max_context_tokens: 2048,
        },
        start,
    );

    let policy = AbortPolicy::new(5);
    let aborted = state.check_abort(&policy, true, start);
    assert_eq!(aborted.signal, AbortSignal::Aborted);
    assert_eq!(aborted.reason, Some(Reason::Aborted));
}

#[test]
fn decide_cancel_or_timeout_prefers_cancel() {
    let now = Instant::now();
    let deadline = now - Duration::from_secs(1);
    let policy = AbortPolicy::new(5);

    let d = policy.check_cancel_or_timeout(true, now, deadline);
    assert_eq!(d.signal, AbortSignal::Aborted);
}

#[test]
fn decide_cancel_or_timeout_handles_timeout_and_none() {
    let now = Instant::now();
    let policy = AbortPolicy::new(5);
    let timeout = policy.check_cancel_or_timeout(false, now, now - Duration::from_secs(1));
    assert_eq!(timeout.signal, AbortSignal::Timeout);

    let none = policy.check_cancel_or_timeout(false, now, now + Duration::from_secs(1));
    assert_eq!(none.signal, AbortSignal::None);
}

#[test]
fn decide_abort_checks_iterations_after_time_cancel() {
    let now = Instant::now();
    let policy = AbortPolicy::new(5);

    let max = policy.check(false, now, now + Duration::from_secs(1), 5);
    assert_eq!(max.signal, AbortSignal::MaxIterations);

    let none = policy.check(false, now, now + Duration::from_secs(1), 4);
    assert_eq!(none.signal, AbortSignal::None);
}

#[test]
fn reason_from_abort_maps_signals() {
    let policy = AbortPolicy::new(1);
    let now = Instant::now();
    let deadline = now + Duration::from_secs(10);

    assert_eq!(policy.check(false, now, deadline, 0).reason, None);
    assert_eq!(
        policy.check(true, now, deadline, 0).reason,
        Some(Reason::Aborted)
    );
    assert_eq!(
        policy
            .check(false, now, now - Duration::from_secs(1), 0)
            .reason,
        Some(Reason::Timeout)
    );
    assert_eq!(
        policy.check(false, now, deadline, 1).reason,
        Some(Reason::MaxIterations)
    );
}

// ── LLMResponse — content_blocks ──

#[test]
fn reasoning_turn_content_blocks_thinking_and_text() {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ThinkingDelta("think".to_string()));
    turn.apply_stream_event(StreamEvent::ContentDelta("answer".to_string()));

    let blocks = turn.content_blocks();
    assert_eq!(blocks.len(), 2);
    assert!(matches!(&blocks[0], ContentBlock::Thinking { thinking } if thinking == "think"));
    assert!(matches!(&blocks[1], ContentBlock::Text { text } if text == "answer"));
}

#[test]
fn reasoning_turn_content_blocks_text_only() {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ContentDelta("hello".to_string()));

    let blocks = turn.content_blocks();
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks[0], ContentBlock::Text { text } if text == "hello"));
}

#[test]
fn reasoning_turn_content_blocks_thinking_only() {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ThinkingDelta("reasoning".to_string()));

    let blocks = turn.content_blocks();
    assert_eq!(blocks.len(), 1);
    assert!(matches!(&blocks[0], ContentBlock::Thinking { thinking } if thinking == "reasoning"));
}

#[test]
fn reasoning_turn_content_blocks_empty() {
    let turn = LLMResponse::new();
    assert!(turn.content_blocks().is_empty());
}

// ── LLMResponse — ttft ──

#[test]
fn reasoning_turn_ttft_ms_roundtrip() {
    let mut turn = LLMResponse::new();
    assert_eq!(turn.ttft_ms(), None);
    turn.set_ttft_ms(42);
    assert_eq!(turn.ttft_ms(), Some(42));
}

// ── LLMResponse — Usage and Error stream events ──

#[test]
fn reasoning_turn_applies_usage_event() {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::Usage(TokenUsage::new(10, 5)));
    assert_eq!(turn.usage().prompt_tokens, 10);
    assert_eq!(turn.usage().completion_tokens, 5);
    assert_eq!(turn.usage().total_tokens, 15);
}

#[test]
fn reasoning_turn_applies_error_event() {
    let mut turn = LLMResponse::new();
    assert!(!turn.has_error());
    turn.apply_stream_event(StreamEvent::Error("oops".to_string()));
    assert!(turn.has_error());
    assert_eq!(turn.take_error().as_deref(), Some("oops"));
}

#[test]
fn reasoning_turn_preview_includes_error() {
    let mut turn = LLMResponse::new();
    turn.apply_stream_event(StreamEvent::ContentDelta("text".to_string()));
    turn.apply_stream_event(StreamEvent::Error("fail".to_string()));
    let preview = turn.response_preview();
    assert!(preview.contains("text"));
    assert!(preview.contains("[error] fail"));
}

// ── RunLoopState — merge_usage, set_ttft ──

#[test]
fn turn_loop_state_merge_usage_accumulates() {
    let mut state = RunLoopState::default();
    state.add_token_usage(&TokenUsage::new(10, 5));

    let extra = bendclaw::kernel::run::result::Usage {
        prompt_tokens: 3,
        completion_tokens: 2,
        total_tokens: 5,
        ..Default::default()
    };
    state.merge_usage(&extra);

    assert_eq!(state.usage().prompt_tokens, 13);
    assert_eq!(state.usage().completion_tokens, 7);
    assert_eq!(state.usage().total_tokens, 20);
}

#[test]
fn turn_loop_state_set_ttft() {
    let mut state = RunLoopState::default();
    assert_eq!(state.usage().ttft_ms, 0);
    state.set_ttft(123);
    assert_eq!(state.usage().ttft_ms, 123);
}
