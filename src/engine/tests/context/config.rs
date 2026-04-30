use evotengine::context::*;
use evotengine::types::*;

#[test]
fn test_estimate_tokens() {
    assert!(estimate_tokens("hello world") > 0);
    assert!(estimate_tokens("hello world") < 10);
    assert_eq!(estimate_tokens(""), 0);
}

#[test]
fn test_context_config_from_context_window() {
    let config = ContextConfig::from_context_window(200_000);
    assert_eq!(config.max_context_tokens, 160_000);
    assert_eq!(config.system_prompt_tokens, 4_000);
    assert_eq!(config.keep_recent, 10);

    let config = ContextConfig::from_context_window(1_000_000);
    assert_eq!(config.max_context_tokens, 800_000);

    let config = ContextConfig::from_context_window(128_000);
    assert_eq!(config.max_context_tokens, 102_400);
}

#[test]
fn test_context_tracker_no_usage() {
    let tracker = ContextTracker::new();
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::user("World")),
    ];
    let tokens = tracker.estimate_context_tokens(&messages);
    assert!(tokens > 0);
    assert_eq!(tokens, total_tokens(&messages));
}

#[test]
fn test_context_tracker_with_usage() {
    let mut tracker = ContextTracker::new();
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::Assistant {
            content: vec![Content::Text {
                text: "Hi there!".into(),
            }],
            stop_reason: StopReason::Stop,
            model: "test".into(),
            provider: "test".into(),
            usage: Usage {
                input: 100,
                output: 50,
                ..Default::default()
            },
            timestamp: 0,
            error_message: None,
        }),
        AgentMessage::Llm(Message::user("Follow up question here")),
    ];
    tracker.record_usage(
        &Usage {
            input: 100,
            output: 50,
            ..Default::default()
        },
        1,
    );
    let tokens = tracker.estimate_context_tokens(&messages);
    let trailing_estimate = message_tokens(&messages[2]);
    // record_usage excludes output tokens: baseline = input(100) only
    assert_eq!(tokens, 100 + trailing_estimate);
}

#[test]
fn test_context_tracker_reset() {
    let mut tracker = ContextTracker::new();
    tracker.record_usage(
        &Usage {
            input: 1000,
            output: 500,
            ..Default::default()
        },
        5,
    );
    tracker.reset();
    let messages = vec![AgentMessage::Llm(Message::user("test"))];
    assert_eq!(
        tracker.estimate_context_tokens(&messages),
        total_tokens(&messages)
    );
}

#[test]
fn test_execution_limits() {
    let limits = ExecutionLimits {
        max_turns: 3,
        max_total_tokens: 1000,
        max_duration: std::time::Duration::from_secs(60),
    };

    let mut tracker = ExecutionTracker::new(limits);
    assert!(tracker.check_limits().is_none());

    tracker.record_turn(100);
    tracker.record_turn(100);
    assert!(tracker.check_limits().is_none());

    tracker.record_turn(100);
    assert!(tracker.check_limits().is_some());
}

#[test]
fn test_context_tracker_record_compaction() {
    let mut tracker = ContextTracker::new();
    // Give tracker a provider baseline at the last message
    tracker.record_usage(
        &Usage {
            input: 50000,
            ..Default::default()
        },
        2,
    );
    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::user("World")),
        AgentMessage::Llm(Message::user("Test")),
    ];
    // Before compaction: baseline dominates
    assert_eq!(tracker.estimate_context_tokens(&messages), 50000);

    // After compaction: baseline is reset, falls back to chars/4
    tracker.record_compaction();
    let tokens = tracker.estimate_context_tokens(&messages);
    assert_eq!(tokens, total_tokens(&messages));
}

#[test]
fn test_context_tracker_record_compaction_with_trailing() {
    let mut tracker = ContextTracker::new();
    // Give tracker a large provider baseline
    tracker.record_usage(
        &Usage {
            input: 176000,
            ..Default::default()
        },
        0,
    );

    // After compaction, reset clears the inflated baseline
    tracker.record_compaction();

    let messages = vec![
        AgentMessage::Llm(Message::user("Hello")),
        AgentMessage::Llm(Message::user("World")),
        AgentMessage::Llm(Message::user("Trailing message after compaction")),
    ];
    // Falls back to pure chars/4 estimation
    let tokens = tracker.estimate_context_tokens(&messages);
    assert_eq!(tokens, total_tokens(&messages));
}

#[test]
fn test_context_tracker_record_compaction_empty() {
    let mut tracker = ContextTracker::new();
    tracker.record_compaction();
    // Falls back to pure estimation
    let messages = vec![AgentMessage::Llm(Message::user("test"))];
    assert_eq!(
        tracker.estimate_context_tokens(&messages),
        total_tokens(&messages)
    );
}

#[test]
fn test_budget_snapshot() {
    let tracker = ContextTracker::new();
    let messages = vec![AgentMessage::Llm(Message::user("Hello"))];
    let config = ContextConfig {
        max_context_tokens: 100_000,
        system_prompt_tokens: 4_000,
        ..Default::default()
    };
    let snapshot = tracker.budget_snapshot(&messages, Some(&config));
    assert_eq!(snapshot.system_prompt_tokens, 4_000);
    assert_eq!(snapshot.budget_tokens, 96_000);
    assert_eq!(snapshot.context_window, 100_000);
    assert_eq!(snapshot.estimated_tokens, total_tokens(&messages));
}

#[test]
fn test_budget_snapshot_no_config() {
    let tracker = ContextTracker::new();
    let messages = vec![AgentMessage::Llm(Message::user("Hello"))];
    let snapshot = tracker.budget_snapshot(&messages, None);
    assert_eq!(snapshot.system_prompt_tokens, 0);
    assert_eq!(snapshot.budget_tokens, 0);
    assert_eq!(snapshot.context_window, 0);
    assert_eq!(snapshot.estimated_tokens, total_tokens(&messages));
}
