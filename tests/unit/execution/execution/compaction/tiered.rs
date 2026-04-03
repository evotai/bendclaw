use std::sync::Arc;

use bendclaw::execution::compaction::build_transcript_from;
use bendclaw::execution::compaction::CompactionConfig;
use bendclaw::execution::compaction::CompactionStrategy;
use bendclaw::execution::compaction::TieredCompactionStrategy;
use bendclaw::sessions::Message;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;
use tokio_util::sync::CancellationToken;

// ── L1: truncate tool outputs ──

#[tokio::test]
async fn l1_truncates_long_tool_output() {
    let long_output = (0..200)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        Message::user("question"),
        Message::assistant("calling tool"),
        Message::tool_result("tc-1", "shell", &long_output, true),
        Message::user("follow up"),
    ];

    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let strategy = TieredCompactionStrategy::new(llm, "mock".into(), CancellationToken::new());
    let config = CompactionConfig {
        max_context_tokens: 999_999, // large budget so L1 alone suffices
        tool_output_max_lines: 80,
        keep_first: 2,
        keep_recent: 10,
    };

    let result = strategy.compact(messages, &config, "run-1").await;
    assert!(result.is_some());
    let outcome = result.unwrap();

    // Find the tool result and verify truncation
    let tool_msg = outcome
        .messages
        .iter()
        .find(|m| matches!(m, Message::ToolResult { .. }))
        .unwrap();
    let text = tool_msg.text();
    assert!(text.contains("lines truncated"));
    let line_count = text.lines().count();
    // Should be roughly 80 lines + the truncation marker line
    assert!(line_count <= 85, "expected ~80 lines, got {line_count}");
}

#[tokio::test]
async fn l1_no_change_when_output_short() {
    let short_output = (0..10)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let messages = vec![
        Message::user("question"),
        Message::tool_result("tc-1", "shell", &short_output, true),
    ];

    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let strategy = TieredCompactionStrategy::new(llm, "mock".into(), CancellationToken::new());
    let config = CompactionConfig {
        max_context_tokens: 999_999,
        tool_output_max_lines: 80,
        keep_first: 2,
        keep_recent: 10,
    };

    // Within budget and no long outputs → None
    let result = strategy.compact(messages, &config, "run-1").await;
    assert!(result.is_none());
}

// ── L2: drop old ToolResult ──

#[tokio::test]
async fn l2_drops_old_tool_results_in_middle() {
    // Build messages: 2 head + 8 middle (with tool results) + 2 tail
    let mut messages = vec![
        Message::user("first user"),
        Message::assistant("first assistant"),
    ];
    for i in 0..4 {
        messages.push(Message::assistant(format!("call tool {i}")));
        messages.push(Message::tool_result(
            format!("tc-{i}"),
            "shell",
            "x ".repeat(500),
            true,
        ));
    }
    messages.push(Message::user("latest question"));
    messages.push(Message::assistant("latest answer"));

    let before_count = messages.len();
    let llm = Arc::new(MockLLMProvider::with_text("summary"));
    let strategy = TieredCompactionStrategy::new(llm, "mock".into(), CancellationToken::new());
    let config = CompactionConfig {
        max_context_tokens: 100,     // very small to force compaction
        tool_output_max_lines: 9999, // disable L1
        keep_first: 2,
        keep_recent: 2,
    };

    let result = strategy.compact(messages, &config, "run-1").await;
    assert!(result.is_some());
    let outcome = result.unwrap();

    // Middle ToolResults should have been dropped (by L2 or L3)
    let tool_count = outcome
        .messages
        .iter()
        .filter(|m| matches!(m, Message::ToolResult { .. }))
        .count();
    assert!(
        tool_count < 4,
        "expected some tool results dropped, still have {tool_count}"
    );
    assert!(outcome.messages.len() < before_count);
}

// ── L3: index mapping with System messages ──

#[tokio::test]
async fn l3_handles_system_messages_in_split_correctly() {
    let big = "token ".repeat(3000);
    let messages = vec![
        Message::system("system prompt"),
        Message::compaction("old summary"),
        Message::user(big.clone()),
        Message::assistant(big.clone()),
        Message::user("recent question"),
        Message::assistant("recent answer"),
    ];

    let llm = Arc::new(MockLLMProvider::with_text("fresh summary"));
    let strategy = TieredCompactionStrategy::new(llm, "mock".into(), CancellationToken::new());
    let config = CompactionConfig {
        // Budget large enough to keep recent messages but not the big ones.
        // keep_budget = min(40000-4000, 10000-4000) = 6000
        max_context_tokens: 10_000,
        tool_output_max_lines: 9999,
        keep_first: 2,
        keep_recent: 10,
    };

    let result = strategy.compact(messages, &config, "run-1").await;
    assert!(result.is_some());
    let outcome = result.unwrap();

    // System and old CompactionSummary should be preserved
    assert!(outcome
        .messages
        .iter()
        .any(|m| matches!(m, Message::System { .. })));
    // Recent messages should be preserved
    assert!(outcome
        .messages
        .iter()
        .any(|m| m.text() == "recent question"));
    assert!(outcome.messages.iter().any(|m| m.text() == "recent answer"));
    // A new CompactionSummary should exist
    let summaries: Vec<_> = outcome
        .messages
        .iter()
        .filter(|m| matches!(m, Message::CompactionSummary { .. }))
        .collect();
    assert!(!summaries.is_empty());
}

/// Proves that L3 split_index → non_system_split mapping is correct
/// when System/CompactionSummary are interleaved in the middle of the
/// message list (not just at the front).
///
/// With max_context_tokens=7000, keep_budget=3000 (7000-4000).
/// Only the two recent small messages fit; both big messages get dropped.
/// The mapping must correctly skip System/CompactionSummary when
/// counting non-system messages before the split point.
#[tokio::test]
async fn l3_split_mapping_with_interleaved_system_messages() {
    let big = "token ".repeat(3000);
    let messages = vec![
        Message::user(big.clone()),
        Message::system("injected system prompt"),
        Message::assistant(big.clone()),
        Message::compaction("prior summary"),
        Message::user("recent question"),
        Message::assistant("recent answer"),
    ];

    let llm = Arc::new(MockLLMProvider::with_text("fresh summary"));
    let strategy = TieredCompactionStrategy::new(llm, "mock".into(), CancellationToken::new());
    let config = CompactionConfig {
        // keep_budget = min(36000, 7000-4000) = 3000
        // Both big messages (~3001 tokens each) exceed this, so both get dropped.
        max_context_tokens: 7_000,
        tool_output_max_lines: 9999,
        keep_first: 2,
        keep_recent: 10,
    };

    let result = strategy.compact(messages, &config, "run-1").await;
    assert!(result.is_some());
    let outcome = result.unwrap();

    // System messages must be preserved
    assert!(
        outcome.messages.iter().any(
            |m| matches!(m, Message::System { content } if content == "injected system prompt")
        ),
        "interleaved System message was lost"
    );

    // Prior CompactionSummary must be preserved
    assert!(
        outcome.messages.iter().any(|m| matches!(m, Message::CompactionSummary { summary, .. } if summary == "prior summary")),
        "prior CompactionSummary was lost"
    );

    // Recent non-system messages must be preserved (not dropped/summarized)
    assert!(
        outcome
            .messages
            .iter()
            .any(|m| m.text() == "recent question"),
        "recent User message was incorrectly dropped"
    );
    assert!(
        outcome.messages.iter().any(|m| m.text() == "recent answer"),
        "recent Assistant message was incorrectly dropped"
    );

    // Big messages should have been summarized away
    assert!(
        !outcome
            .messages
            .iter()
            .any(|m| m.text().starts_with("token token token")),
        "big messages should have been dropped and summarized"
    );

    // A new CompactionSummary should exist (from L3 summarization)
    let new_summaries: Vec<_> = outcome.messages.iter()
        .filter(|m| matches!(m, Message::CompactionSummary { summary, .. } if summary == "fresh summary"))
        .collect();
    assert_eq!(new_summaries.len(), 1, "expected exactly one new summary");
}

// ── build_transcript prompt-relevant filtering ──

#[test]
fn build_transcript_excludes_non_prompt_relevant_messages() {
    let messages = vec![
        Message::user("user question"),
        Message::assistant("assistant answer"),
        Message::Memory {
            operation: "extract".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::note("internal note"),
        Message::operation_event("llm", "turn", "done", serde_json::json!({})),
        Message::tool_result("tc-1", "shell", "output", true),
    ];

    let transcript = build_transcript_from(&messages);

    // Prompt-relevant messages should appear
    assert!(transcript.contains("user question"), "user message missing");
    assert!(
        transcript.contains("assistant answer"),
        "assistant message missing"
    );
    assert!(transcript.contains("output"), "tool result missing");

    // Non-prompt-relevant messages should NOT appear
    assert!(
        !transcript.contains("k: v"),
        "Memory should be filtered out"
    );
    assert!(
        !transcript.contains("internal note"),
        "Note should be filtered out"
    );
    assert!(
        !transcript.contains("llm"),
        "OperationEvent should be filtered out"
    );
}

#[test]
fn build_transcript_empty_for_only_non_relevant() {
    let messages = vec![
        Message::Memory {
            operation: "extract".into(),
            key: "k".into(),
            value: "v".into(),
        },
        Message::note("note"),
    ];

    let transcript = build_transcript_from(&messages);
    assert!(
        transcript.is_empty(),
        "transcript should be empty for non-relevant messages"
    );
}
