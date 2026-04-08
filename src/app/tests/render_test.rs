use bendclaw::cli::repl::render::count_messages_by_role;
use bendclaw::cli::repl::render::format_llm_call_lines;
use bendclaw::cli::repl::render::format_llm_completed_lines;
use bendclaw::cli::repl::render::tool_result_lines;
use bendclaw::cli::repl::render::ToolCallSummary;

#[test]
fn tool_result_lines_preserves_multiline_content() {
    let lines = tool_result_lines("line 1\nline 2\n\nline 4\n", false, None);
    assert_eq!(lines, vec!["line 1", "line 2", "", "line 4"]);
}

#[test]
fn tool_result_lines_keeps_single_line_summary_behavior() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines("full file contents", false, Some(&tool_call));
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}

#[test]
fn tool_result_lines_keeps_read_results_compact_even_when_multiline() {
    let tool_call = ToolCallSummary {
        name: "read_file".into(),
        summary: "/tmp/demo.txt".into(),
    };
    let lines = tool_result_lines(
        "[20 lines]\n   1 | first\n   2 | second",
        false,
        Some(&tool_call),
    );
    assert_eq!(lines, vec!["Result: /tmp/demo.txt"]);
}

// ---------------------------------------------------------------------------
// count_messages_by_role
// ---------------------------------------------------------------------------

#[test]
fn count_messages_by_role_splits_by_role() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "hello"}),
        serde_json::json!({"role": "assistant", "content": "hi there"}),
        serde_json::json!({"role": "user", "content": "do something"}),
        serde_json::json!({"role": "toolResult", "content": "file contents here"}),
        serde_json::json!({"role": "toolResult", "content": "search results"}),
    ];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.user_count, 2);
    assert_eq!(stats.assistant_count, 1);
    assert_eq!(stats.tool_result_count, 2);
    assert_eq!(stats.total_count(), 5);
    assert!(stats.user_tokens > 0);
    assert!(stats.assistant_tokens > 0);
    assert!(stats.tool_result_tokens > 0);
}

#[test]
fn count_messages_by_role_empty() {
    let stats = count_messages_by_role(&[]);
    assert_eq!(stats.total_count(), 0);
    assert_eq!(stats.total_tokens(100), 100);
}

#[test]
fn count_messages_by_role_unknown_role_counts_as_user() {
    let messages: Vec<serde_json::Value> =
        vec![serde_json::json!({"role": "system", "content": "you are helpful"})];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.user_count, 1);
    assert_eq!(stats.assistant_count, 0);
    assert_eq!(stats.tool_result_count, 0);
}

#[test]
fn count_messages_by_role_handles_tool_variant_names() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "tool_result", "content": "a"}),
        serde_json::json!({"role": "tool", "content": "b"}),
        serde_json::json!({"role": "toolResult", "content": "c"}),
    ];
    let stats = count_messages_by_role(&messages);
    assert_eq!(stats.tool_result_count, 3);
}

// ---------------------------------------------------------------------------
// format_llm_call_lines
// ---------------------------------------------------------------------------

#[test]
fn format_llm_call_lines_basic() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "hello world"}),
        serde_json::json!({"role": "assistant", "content": "hi"}),
    ];
    let stats = count_messages_by_role(&messages);
    let lines = format_llm_call_lines(&stats, 3, 495);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("2 messages"));
    assert!(msg_line.contains("user 1"));
    assert!(msg_line.contains("assistant 1"));
    assert!(!msg_line.contains("tool_result"));
    assert!(msg_line.contains("3 tools"));

    assert!(token_line.contains("est tokens"));
    assert!(token_line.contains("sys ~495"));
    assert!(token_line.contains("user ~"));
    assert!(token_line.contains("assistant ~"));
    assert!(!token_line.contains("tool_result"));
}

#[test]
fn format_llm_call_lines_with_tool_results() {
    let messages: Vec<serde_json::Value> = vec![
        serde_json::json!({"role": "user", "content": "read the file"}),
        serde_json::json!({"role": "assistant", "content": "sure"}),
        serde_json::json!({"role": "toolResult", "toolName": "read", "content": "file data here"}),
    ];
    let stats = count_messages_by_role(&messages);
    let lines = format_llm_call_lines(&stats, 6, 500);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("3 messages"));
    assert!(msg_line.contains("tool_result 1"));
    assert!(msg_line.contains("6 tools"));

    assert!(token_line.contains("tool_result ~"));
}

#[test]
fn format_llm_call_lines_empty_messages() {
    let stats = count_messages_by_role(&[]);
    let lines = format_llm_call_lines(&stats, 0, 200);

    let msg_line = &lines[0];
    let token_line = &lines[1];

    assert!(msg_line.contains("0 messages"));
    assert!(msg_line.contains("0 tools"));
    assert!(token_line.contains("~200 est tokens"));
    assert!(token_line.contains("sys ~200"));
}

#[test]
fn tool_result_lines_truncates_large_output() {
    let big_content: String = (0..100)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = tool_result_lines(&big_content, false, None);
    assert_eq!(lines.len(), 31); // 30 lines + 1 truncation notice
    assert!(lines[30].contains("70 more lines truncated"));
}

#[test]
fn tool_result_lines_no_truncation_under_limit() {
    let content: String = (0..20)
        .map(|i| format!("line {i}"))
        .collect::<Vec<_>>()
        .join("\n");
    let lines = tool_result_lines(&content, false, None);
    assert_eq!(lines.len(), 20);
}

// ---------------------------------------------------------------------------
// format_llm_completed_lines
// ---------------------------------------------------------------------------

#[test]
fn format_llm_completed_lines_without_metrics() {
    let usage = bendclaw::protocol::UsageSummary {
        input: 61001,
        output: 248,
        cache_read: 0,
        cache_write: 0,
    };

    let lines = format_llm_completed_lines(&usage, None);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0], "tokens   61001 in · 248 out");
}

#[test]
fn format_llm_completed_lines_with_metrics_and_throughput() {
    let usage = bendclaw::protocol::UsageSummary {
        input: 61001,
        output: 248,
        cache_read: 0,
        cache_write: 0,
    };
    let metrics = bendclaw::protocol::LlmCallMetrics {
        duration_ms: 3200,
        ttfb_ms: 245,
        ttft_ms: 892,
        streaming_ms: 2308,
        chunk_count: 12,
    };

    let lines = format_llm_completed_lines(&usage, Some(&metrics));
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("61001 in · 248 out"));
    assert!(lines[0].contains("tok/s"));
    assert!(lines[1].contains("timing   3.2s"));
    assert!(lines[1].contains("ttfb 245ms"));
    assert!(lines[1].contains("ttft 892ms"));
    assert!(lines[1].contains("stream 2.3s"));
}

#[test]
fn format_llm_completed_lines_skips_throughput_when_streaming_missing() {
    let usage = bendclaw::protocol::UsageSummary {
        input: 200,
        output: 80,
        cache_read: 0,
        cache_write: 0,
    };
    let metrics = bendclaw::protocol::LlmCallMetrics {
        duration_ms: 900,
        ttfb_ms: 120,
        ttft_ms: 0,
        streaming_ms: 0,
        chunk_count: 0,
    };

    let lines = format_llm_completed_lines(&usage, Some(&metrics));
    assert_eq!(lines.len(), 2);
    assert!(!lines[0].contains("tok/s"));
    assert!(lines[1].contains("900ms"));
    assert!(lines[1].contains("ttfb 120ms"));
    assert!(!lines[1].contains("ttft"));
    assert!(!lines[1].contains("stream"));
}

// ---------------------------------------------------------------------------
// format_run_summary
// ---------------------------------------------------------------------------

use bendclaw::cli::repl::render::format_run_summary;
use bendclaw::cli::repl::render::CompactRecord;
use bendclaw::cli::repl::render::MessageStats;
use bendclaw::cli::repl::render::RunSummaryData;
use bendclaw::cli::repl::render::ToolAggStats;

fn make_summary_data() -> RunSummaryData {
    RunSummaryData {
        duration_ms: 226500,
        turn_count: 11,
        usage: bendclaw::protocol::UsageSummary {
            input: 750142,
            output: 1796,
            cache_read: 710000,
            cache_write: 38346,
        },
        llm_call_count: 11,
        tool_call_count: 10,
        system_prompt_tokens: 12800,
        last_message_stats: Some(MessageStats {
            user_count: 11,
            assistant_count: 10,
            tool_result_count: 10,
            user_tokens: 8200,
            assistant_tokens: 25000,
            tool_result_tokens: 702346,
            tool_details: vec![],
        }),
        llm_metrics: vec![
            bendclaw::protocol::LlmCallMetrics {
                duration_ms: 41200,
                ttfb_ms: 300,
                ttft_ms: 1800,
                streaming_ms: 39000,
                chunk_count: 50,
            },
            bendclaw::protocol::LlmCallMetrics {
                duration_ms: 22800,
                ttfb_ms: 280,
                ttft_ms: 1400,
                streaming_ms: 21000,
                chunk_count: 30,
            },
        ],
        llm_output_tokens: vec![900, 896],
        tool_stats: vec![
            ("read_file".into(), ToolAggStats {
                calls: 5,
                result_tokens: 312000,
                duration_ms: 12300,
                errors: 0,
            }),
            ("search".into(), ToolAggStats {
                calls: 3,
                result_tokens: 98000,
                duration_ms: 8100,
                errors: 0,
            }),
        ],
        compact_history: vec![CompactRecord {
            level: 1,
            before_tokens: 320000,
            after_tokens: 180000,
        }],
    }
}

#[test]
fn format_run_summary_contains_header() {
    let data = make_summary_data();
    let lines = format_run_summary(&data);
    assert!(lines[0].contains("This Run Summary"));
    assert!(lines[1].contains("226.5s"));
    assert!(lines[1].contains("11 turns"));
    assert!(lines[1].contains("11 llm calls"));
    assert!(lines[1].contains("10 tool calls"));
}

#[test]
fn format_run_summary_contains_token_breakdown() {
    let data = make_summary_data();
    let lines = format_run_summary(&data);
    let all = lines.join("\n");
    assert!(all.contains("system"));
    assert!(all.contains("user"));
    assert!(all.contains("assistant"));
    assert!(all.contains("tool_result"));
    assert!(all.contains("read_file"));
    assert!(all.contains("5 calls"));
    assert!(all.contains("search"));
    assert!(all.contains("3 calls"));
}

#[test]
fn format_run_summary_contains_compact() {
    let data = make_summary_data();
    let lines = format_run_summary(&data);
    let all = lines.join("\n");
    assert!(all.contains("compact"));
    assert!(all.contains("lv1"));
    assert!(all.contains("320k"));
    assert!(all.contains("180k"));
}

#[test]
fn format_run_summary_contains_llm_block() {
    let data = make_summary_data();
    let lines = format_run_summary(&data);
    let all = lines.join("\n");
    assert!(all.contains("2 calls"));
    assert!(all.contains("ttft avg"));
    assert!(all.contains("stream avg"));
    assert!(all.contains("#1"));
    assert!(all.contains("#2"));
}

#[test]
fn format_run_summary_no_compact_when_empty() {
    let mut data = make_summary_data();
    data.compact_history.clear();
    let lines = format_run_summary(&data);
    let all = lines.join("\n");
    assert!(!all.contains("compact"));
}

#[test]
fn format_run_summary_llm_bars_are_aligned() {
    // Use metrics with varying index widths (#1 vs #12) and duration widths (8.5s vs 46.9s)
    let mut data = make_summary_data();
    let base_metric = bendclaw::protocol::LlmCallMetrics {
        duration_ms: 5000,
        ttfb_ms: 200,
        ttft_ms: 800,
        streaming_ms: 4000,
        chunk_count: 10,
    };
    // 12 calls so indices range from #1 to #12
    data.llm_metrics = (0..12)
        .map(|i| bendclaw::protocol::LlmCallMetrics {
            duration_ms: if i == 0 {
                46900
            } else if i == 4 {
                8500
            } else {
                3000 + i as u64 * 100
            },
            ..base_metric
        })
        .collect();
    data.llm_output_tokens = vec![100; 12];
    data.llm_call_count = 12;

    let lines = format_run_summary(&data);

    // Find the top-3 LLM call lines (they start with spaces + '#' and contain bar + percentage)
    let bar_lines: Vec<&String> = lines
        .iter()
        .filter(|l| {
            let trimmed = l.trim_start();
            trimmed.starts_with('#') && l.contains('█') && !l.contains("lv")
        })
        .collect();
    assert_eq!(
        bar_lines.len(),
        3,
        "expected 3 bar lines, got: {bar_lines:?}"
    );

    // The bar character '█' must start at the same column in each line
    let bar_positions: Vec<usize> = bar_lines
        .iter()
        .map(|l| l.find('█').expect("bar char not found"))
        .collect();
    assert!(
        bar_positions.windows(2).all(|w| w[0] == w[1]),
        "bar positions not aligned: {bar_positions:?}\n{}\n{}\n{}",
        bar_lines[0],
        bar_lines[1],
        bar_lines[2],
    );
}

#[test]
fn format_run_summary_tool_stats_bars_are_aligned() {
    let data = make_summary_data();
    let lines = format_run_summary(&data);

    // Tool stat lines contain tool names from the fixture: read_file, search
    let tool_lines: Vec<&String> = lines
        .iter()
        .filter(|l| (l.contains("read_file") || l.contains("search")) && l.contains('█'))
        .collect();
    assert_eq!(
        tool_lines.len(),
        2,
        "expected 2 tool lines, got: {tool_lines:?}"
    );

    let bar_positions: Vec<usize> = tool_lines
        .iter()
        .map(|l| l.find('█').expect("bar char not found"))
        .collect();
    assert!(
        bar_positions.windows(2).all(|w| w[0] == w[1]),
        "tool bar positions not aligned: {bar_positions:?}\n{}\n{}",
        tool_lines[0],
        tool_lines[1],
    );
}
