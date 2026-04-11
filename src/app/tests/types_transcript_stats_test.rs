use bendclaw::types::observability::*;
use bendclaw::types::*;

// ---------------------------------------------------------------------------
// TranscriptStats serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn stats_llm_call_started_round_trip() {
    let stats = TranscriptStats::LlmCallStarted(LlmCallStartedStats {
        turn: 1,
        attempt: 0,
        model: "claude-3".into(),
        message_count: 5,
        message_bytes: 1200,
        system_prompt_tokens: 300,
    });
    let item = stats.to_item();
    assert!(matches!(&item, TranscriptItem::Stats { kind, .. } if kind == "llm_call_started"));

    let decoded = TranscriptStats::try_from_item(&item);
    assert!(decoded.is_some());
    if let Some(TranscriptStats::LlmCallStarted(s)) = decoded {
        assert_eq!(s.turn, 1);
        assert_eq!(s.model, "claude-3");
        assert_eq!(s.message_count, 5);
    } else {
        panic!("expected LlmCallStarted");
    }
}

#[test]
fn stats_llm_call_completed_round_trip() {
    let stats = TranscriptStats::LlmCallCompleted(LlmCallCompletedStats {
        turn: 2,
        attempt: 1,
        usage: UsageSummary {
            input: 1000,
            output: 200,
            cache_read: 50,
            cache_write: 10,
        },
        metrics: Some(LlmCallMetrics {
            duration_ms: 3000,
            ttfb_ms: 200,
            ttft_ms: 500,
            streaming_ms: 2500,
            chunk_count: 42,
        }),
        error: None,
    });
    let item = stats.to_item();
    let decoded = TranscriptStats::try_from_item(&item);
    assert!(decoded.is_some());
    if let Some(TranscriptStats::LlmCallCompleted(s)) = decoded {
        assert_eq!(s.usage.input, 1000);
        assert_eq!(s.usage.output, 200);
        assert!(s.metrics.is_some());
        assert_eq!(s.metrics.as_ref().map(|m| m.ttft_ms), Some(500));
    } else {
        panic!("expected LlmCallCompleted");
    }
}

#[test]
fn stats_tool_finished_round_trip() {
    let stats = TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc1".into(),
        tool_name: "read_file".into(),
        result_tokens: 150,
        duration_ms: 80,
        is_error: false,
    });
    let item = stats.to_item();
    let decoded = TranscriptStats::try_from_item(&item);
    if let Some(TranscriptStats::ToolFinished(s)) = decoded {
        assert_eq!(s.tool_name, "read_file");
        assert_eq!(s.result_tokens, 150);
        assert!(!s.is_error);
    } else {
        panic!("expected ToolFinished");
    }
}

#[test]
fn stats_context_compaction_started_round_trip() {
    let stats = TranscriptStats::ContextCompactionStarted(ContextCompactionStartedStats {
        message_count: 20,
        estimated_tokens: 50000,
        budget_tokens: 80000,
        system_prompt_tokens: 5000,
        context_window: 100000,
    });
    let item = stats.to_item();
    let decoded = TranscriptStats::try_from_item(&item);
    if let Some(TranscriptStats::ContextCompactionStarted(s)) = decoded {
        assert_eq!(s.message_count, 20);
        assert_eq!(s.estimated_tokens, 50000);
    } else {
        panic!("expected ContextCompactionStarted");
    }
}

#[test]
fn stats_context_compaction_completed_round_trip() {
    let stats = TranscriptStats::ContextCompactionCompleted(ContextCompactionCompletedStats {
        level: 2,
        before_message_count: 20,
        after_message_count: 8,
        before_estimated_tokens: 50000,
        after_estimated_tokens: 20000,
        tool_outputs_truncated: 3,
        turns_summarized: 5,
        messages_dropped: 4,
        actions: vec![],
    });
    let item = stats.to_item();
    let decoded = TranscriptStats::try_from_item(&item);
    if let Some(TranscriptStats::ContextCompactionCompleted(s)) = decoded {
        assert_eq!(s.level, 2);
        assert_eq!(s.before_estimated_tokens, 50000);
        assert_eq!(s.after_estimated_tokens, 20000);
    } else {
        panic!("expected ContextCompactionCompleted");
    }
}

#[test]
fn stats_run_finished_round_trip() {
    let stats = TranscriptStats::RunFinished(RunFinishedStats {
        usage: UsageSummary {
            input: 5000,
            output: 1000,
            cache_read: 200,
            cache_write: 50,
        },
        turn_count: 3,
        duration_ms: 12000,
        transcript_count: 15,
    });
    let item = stats.to_item();
    let decoded = TranscriptStats::try_from_item(&item);
    if let Some(TranscriptStats::RunFinished(s)) = decoded {
        assert_eq!(s.usage.input, 5000);
        assert_eq!(s.turn_count, 3);
        assert_eq!(s.duration_ms, 12000);
    } else {
        panic!("expected RunFinished");
    }
}

// ---------------------------------------------------------------------------
// try_from_item edge cases
// ---------------------------------------------------------------------------

#[test]
fn try_from_item_returns_none_for_non_stats() {
    let item = TranscriptItem::User {
        text: "hello".into(),
    };
    assert!(TranscriptStats::try_from_item(&item).is_none());
}

#[test]
fn try_from_item_returns_none_for_unknown_kind() {
    let item = TranscriptItem::Stats {
        kind: "unknown_future_kind".into(),
        data: serde_json::json!({"foo": "bar"}),
    };
    assert!(TranscriptStats::try_from_item(&item).is_none());
}

#[test]
fn try_from_item_returns_none_for_schema_mismatch() {
    let item = TranscriptItem::Stats {
        kind: "llm_call_started".into(),
        data: serde_json::json!({"wrong_field": true}),
    };
    assert!(TranscriptStats::try_from_item(&item).is_none());
}

// ---------------------------------------------------------------------------
// is_context_item
// ---------------------------------------------------------------------------

#[test]
fn stats_item_is_not_context() {
    let item = TranscriptStats::RunFinished(RunFinishedStats {
        usage: UsageSummary::default(),
        turn_count: 1,
        duration_ms: 100,
        transcript_count: 2,
    })
    .to_item();
    assert!(!item.is_context_item());
}

#[test]
fn user_item_is_context() {
    let item = TranscriptItem::User {
        text: "hello".into(),
    };
    assert!(item.is_context_item());
}

#[test]
fn compact_item_is_not_context() {
    let item = TranscriptItem::Compact {
        messages: vec![TranscriptItem::User { text: "hi".into() }],
    };
    assert!(!item.is_context_item());
}

// ---------------------------------------------------------------------------
// JSONL serialization stability
// ---------------------------------------------------------------------------

#[test]
fn stats_item_serializes_to_flat_jsonl() {
    let stats = TranscriptStats::ToolFinished(ToolFinishedStats {
        tool_call_id: "tc1".into(),
        tool_name: "bash".into(),
        result_tokens: 42,
        duration_ms: 100,
        is_error: false,
    });
    let item = stats.to_item();
    let json = serde_json::to_string(&item).expect("serialize");
    // Should contain type=stats and kind at top level
    assert!(json.contains(r#""type":"stats""#));
    assert!(json.contains(r#""kind":"tool_finished""#));
    // data should contain the tool fields
    assert!(json.contains(r#""tool_name":"bash""#));
}

#[test]
fn stats_item_deserializes_from_jsonl() {
    let json = r#"{"type":"stats","kind":"run_finished","data":{"usage":{"input":100,"output":50,"cache_read":0,"cache_write":0},"turn_count":2,"duration_ms":1500,"transcript_count":4}}"#;
    let item: TranscriptItem = serde_json::from_str(json).expect("deserialize");
    assert!(matches!(&item, TranscriptItem::Stats { kind, .. } if kind == "run_finished"));
    let decoded = TranscriptStats::try_from_item(&item);
    if let Some(TranscriptStats::RunFinished(s)) = decoded {
        assert_eq!(s.usage.input, 100);
        assert_eq!(s.turn_count, 2);
    } else {
        panic!("expected RunFinished");
    }
}
