use bendclaw::session::observability::StatsAggregator;
use bendclaw::types::observability::*;
use bendclaw::types::*;

// ---------------------------------------------------------------------------
// Basic ingest + summary
// ---------------------------------------------------------------------------

#[test]
fn aggregator_empty_summary() {
    let mut agg = StatsAggregator::new();
    let usage = UsageSummary::default();
    let summary = agg.to_run_summary(0, 0, &usage);
    assert_eq!(summary.llm_call_count, 0);
    assert_eq!(summary.tool_call_count, 0);
    assert!(summary.llm_metrics.is_empty());
    assert!(summary.tool_stats.is_empty());
    assert!(summary.compact_history.is_empty());
}

#[test]
fn aggregator_ingests_llm_call_started() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 5,
        message_bytes: 1200,
        system_prompt_tokens: 300,
    }));
    assert_eq!(agg.llm_call_count, 1);
    assert_eq!(agg.system_prompt_tokens, 300);
    assert_eq!(agg.last_model.as_deref(), Some("claude-3"));
}

#[test]
fn aggregator_ingests_llm_call_completed() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
        turn: 1,
        attempt: 0,
        usage: UsageSummary {
            input: 1000,
            output: 200,
            cache_read: 0,
            cache_write: 0,
        },
        metrics: Some(LlmCallMetrics {
            duration_ms: 3000,
            ttfb_ms: 200,
            ttft_ms: 500,
            streaming_ms: 2500,
            chunk_count: 42,
        }),
        error: None,
    }));
    assert_eq!(agg.llm_metrics.len(), 1);
    assert_eq!(agg.llm_output_tokens, vec![200]);
}

#[test]
fn aggregator_ingests_tool_finished() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc1".into(),
        tool_name: "read_file".into(),
        result_tokens: 150,
        duration_ms: 80,
        is_error: false,
    }));
    agg.ingest(&TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc2".into(),
        tool_name: "read_file".into(),
        result_tokens: 200,
        duration_ms: 120,
        is_error: false,
    }));
    agg.ingest(&TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc3".into(),
        tool_name: "bash".into(),
        result_tokens: 50,
        duration_ms: 500,
        is_error: true,
    }));

    assert_eq!(agg.tool_call_count, 3);

    let rf = agg.tool_stats.get("read_file");
    assert!(rf.is_some());
    let rf = rf.unwrap();
    assert_eq!(rf.calls, 2);
    assert_eq!(rf.result_tokens, 350);
    assert_eq!(rf.errors, 0);

    let bash = agg.tool_stats.get("bash");
    assert!(bash.is_some());
    let bash = bash.unwrap();
    assert_eq!(bash.calls, 1);
    assert_eq!(bash.errors, 1);
}

#[test]
fn aggregator_ingests_compaction_completed() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::ContextCompactionCompleted(
        ContextCompactionCompletedStats {
            level: 1,
            before_message_count: 20,
            after_message_count: 10,
            before_estimated_tokens: 50000,
            after_estimated_tokens: 25000,
            tool_outputs_truncated: 3,
            turns_summarized: 5,
            messages_dropped: 2,
            actions: vec![],
        },
    ));
    assert_eq!(agg.compact_history.len(), 1);
    assert_eq!(agg.compact_history[0].level, 1);
    assert_eq!(agg.compact_history[0].before_tokens, 50000);
    assert_eq!(agg.compact_history[0].after_tokens, 25000);
}

#[test]
fn aggregator_ingests_run_finished() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::RunFinished(RunFinishedStats {
        usage: UsageSummary {
            input: 5000,
            output: 1000,
            cache_read: 200,
            cache_write: 50,
        },
        turn_count: 3,
        duration_ms: 12000,
        transcript_count: 15,
    }));
    assert_eq!(agg.run_duration_ms, Some(12000));
    assert_eq!(agg.run_turn_count, Some(3));
    assert!(agg.run_usage.is_some());
}

// ---------------------------------------------------------------------------
// to_run_summary
// ---------------------------------------------------------------------------

#[test]
fn aggregator_to_run_summary_produces_correct_data() {
    let mut agg = StatsAggregator::new();

    // Simulate a run with 2 LLM calls and 1 tool call
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 3,
        message_bytes: 500,
        system_prompt_tokens: 200,
    }));
    agg.ingest(&TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
        turn: 1,
        attempt: 0,
        usage: UsageSummary {
            input: 500,
            output: 100,
            cache_read: 0,
            cache_write: 0,
        },
        metrics: Some(LlmCallMetrics {
            duration_ms: 2000,
            ttfb_ms: 100,
            ttft_ms: 300,
            streaming_ms: 1700,
            chunk_count: 20,
        }),
        error: None,
    }));
    agg.ingest(&TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        result_tokens: 50,
        duration_ms: 300,
        is_error: false,
    }));
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 5,
        message_bytes: 800,
        system_prompt_tokens: 200,
    }));
    agg.ingest(&TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
        turn: 1,
        attempt: 0,
        usage: UsageSummary {
            input: 800,
            output: 150,
            cache_read: 0,
            cache_write: 0,
        },
        metrics: Some(LlmCallMetrics {
            duration_ms: 1500,
            ttfb_ms: 80,
            ttft_ms: 200,
            streaming_ms: 1300,
            chunk_count: 15,
        }),
        error: None,
    }));

    let usage = UsageSummary {
        input: 1300,
        output: 250,
        cache_read: 0,
        cache_write: 0,
    };
    let summary = agg.to_run_summary(5000, 1, &usage);

    assert_eq!(summary.llm_call_count, 2);
    assert_eq!(summary.tool_call_count, 1);
    assert_eq!(summary.system_prompt_tokens, 200);
    assert_eq!(summary.llm_metrics.len(), 2);
    assert_eq!(summary.llm_output_tokens, vec![100, 150]);
    assert_eq!(summary.tool_stats.len(), 1);
    assert_eq!(summary.tool_stats[0].0, "bash");
    assert_eq!(summary.duration_ms, 5000);
    assert_eq!(summary.turn_count, 1);
    assert!(summary.last_message_stats.is_none());
}

// ---------------------------------------------------------------------------
// to_run_summary_from_stats (offline path)
// ---------------------------------------------------------------------------

#[test]
fn aggregator_to_run_summary_from_stats_returns_none_without_run_finished() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 3,
        message_bytes: 500,
        system_prompt_tokens: 200,
    }));
    assert!(agg.to_run_summary_from_stats().is_none());
}

#[test]
fn aggregator_to_run_summary_from_stats_works_with_run_finished() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 3,
        message_bytes: 500,
        system_prompt_tokens: 200,
    }));
    agg.ingest(&TranscriptStats::RunFinished(RunFinishedStats {
        usage: UsageSummary {
            input: 500,
            output: 100,
            cache_read: 0,
            cache_write: 0,
        },
        turn_count: 1,
        duration_ms: 3000,
        transcript_count: 5,
    }));
    let summary = agg.to_run_summary_from_stats();
    assert!(summary.is_some());
    let summary = summary.unwrap();
    assert_eq!(summary.duration_ms, 3000);
    assert_eq!(summary.turn_count, 1);
    assert_eq!(summary.usage.input, 500);
}

// ---------------------------------------------------------------------------
// from_items (batch ingest)
// ---------------------------------------------------------------------------

#[test]
fn aggregator_from_items_batch_ingest() {
    let items = vec![
        TranscriptItem::User {
            text: "hello".into(),
        },
        TranscriptStats::LlmCallStarted(LlmCallStartedStats {
            turn: 1,
            attempt: 0,
            model: "claude-3".into(),
            message_count: 1,
            message_bytes: 100,
            system_prompt_tokens: 50,
        })
        .to_item(),
        TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
            turn: 1,
            attempt: 0,
            usage: UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            },
            metrics: None,
            error: None,
        })
        .to_item(),
        TranscriptItem::Assistant {
            text: "hi".into(),
            thinking: None,
            tool_calls: vec![],
            stop_reason: "end_turn".into(),
        },
        TranscriptStats::RunFinished(RunFinishedStats {
            usage: UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            },
            turn_count: 1,
            duration_ms: 2000,
            transcript_count: 3,
        })
        .to_item(),
    ];

    let agg = StatsAggregator::from_items(&items);
    assert_eq!(agg.llm_call_count, 1);
    assert_eq!(agg.run_duration_ms, Some(2000));
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

#[test]
fn aggregator_reset_clears_state() {
    let mut agg = StatsAggregator::new();
    agg.ingest(&TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 3,
        message_bytes: 500,
        system_prompt_tokens: 200,
    }));
    assert_eq!(agg.llm_call_count, 1);

    agg.reset();
    assert_eq!(agg.llm_call_count, 0);
    assert!(agg.last_model.is_none());
    assert_eq!(agg.system_prompt_tokens, 0);
}
