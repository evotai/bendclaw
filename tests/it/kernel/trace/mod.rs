pub mod span_meta;

use anyhow::Result;
use bendclaw::kernel::trace::SpanMeta;
use bendclaw::kernel::Impact;
use bendclaw::storage::AgentTraceBreakdown;
use bendclaw::storage::AgentTraceDetails;
use bendclaw::storage::AgentTraceSummary;
use bendclaw::storage::SpanRecord;

#[test]
fn span_record_serde_roundtrip() -> Result<()> {
    let record = SpanRecord {
        span_id: "e1".into(),
        trace_id: "t1".into(),
        parent_span_id: String::new(),
        kind: "llm".into(),
        name: "reasoning.turn".into(),
        model_role: String::new(),
        status: "completed".into(),
        duration_ms: 500,
        ttft_ms: 0,
        input_tokens: 100,
        output_tokens: 50,
        reasoning_tokens: 0,
        cost: 0.01,
        error_code: String::new(),
        error_message: String::new(),
        summary: "llm reasoning completed".into(),
        meta: "{}".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record)?;
    let back: SpanRecord = serde_json::from_str(&json)?;
    assert_eq!(back.span_id, "e1");
    assert_eq!(back.trace_id, "t1");
    assert_eq!(back.duration_ms, 500);
    assert_eq!(back.input_tokens, 100);
    assert_eq!(back.cost, 0.01);
    Ok(())
}

#[test]
fn trace_summary_default() {
    let s = AgentTraceSummary::default();
    assert!(s.agent_id.is_empty());
    assert_eq!(s.trace_count, 0);
    assert_eq!(s.llm_calls, 0);
    assert_eq!(s.total_cost, 0.0);
}

#[test]
fn trace_breakdown_default() {
    let b = AgentTraceBreakdown::default();
    assert!(b.name.is_empty());
    assert_eq!(b.calls, 0);
    assert_eq!(b.errors, 0);
}

#[test]
fn trace_details_default() {
    let d = AgentTraceDetails::default();
    assert!(d.llm.is_empty());
    assert!(d.tools.is_empty());
    assert!(d.skills.is_empty());
    assert!(d.errors.is_empty());
    assert!(d.recent_trace_ids.is_empty());
}

#[test]
fn trace_summary_serialize() -> Result<()> {
    let s = AgentTraceSummary {
        agent_id: "agent1".into(),
        trace_count: 5,
        llm_calls: 10,
        tool_calls: 3,
        skill_calls: 1,
        error_count: 0,
        input_tokens: 1000,
        output_tokens: 500,
        total_cost: 0.05,
        avg_duration_ms: 200.0,
        last_active: "2026-01-01".into(),
    };
    let json = serde_json::to_string(&s)?;
    assert!(json.contains("\"agent_id\":\"agent1\""));
    assert!(json.contains("\"trace_count\":5"));
    Ok(())
}

// ── SpanMeta ──

#[test]
fn span_meta_llm_turn_json() -> Result<()> {
    let meta = SpanMeta::LlmTurn { iteration: 3 };
    let json = meta.to_json();
    let parsed: serde_json::Value = serde_json::from_str(&json)?;
    assert_eq!(parsed["iteration"], 3);
    Ok(())
}

#[test]
fn span_meta_llm_completed_json() -> Result<()> {
    let meta = SpanMeta::LlmCompleted {
        finish_reason: "end_turn".into(),
        prompt_tokens: 100,
        completion_tokens: 50,
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["finish_reason"], "end_turn");
    assert_eq!(parsed["prompt_tokens"], 100);
    assert_eq!(parsed["completion_tokens"], 50);
    Ok(())
}

#[test]
fn span_meta_llm_failed_json() -> Result<()> {
    let meta = SpanMeta::LlmFailed {
        finish_reason: "error".into(),
        error: "rate limit".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["error"], "rate limit");
    Ok(())
}

#[test]
fn span_meta_tool_started_json() -> Result<()> {
    let meta = SpanMeta::ToolStarted {
        tool_call_id: "tc_001".into(),
        arguments: serde_json::json!({"cmd": "ls"}),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["tool_call_id"], "tc_001");
    assert_eq!(parsed["arguments"]["cmd"], "ls");
    Ok(())
}

#[test]
fn span_meta_tool_completed_with_impact() -> Result<()> {
    let meta = SpanMeta::ToolCompleted {
        tool_call_id: "tc_002".into(),
        duration_ms: 42,
        impact: Some(Impact::Low),
        summary: "read file".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["duration_ms"], 42);
    assert_eq!(parsed["impact"], "Low");
    Ok(())
}

#[test]
fn span_meta_tool_completed_no_impact_skips_field() -> Result<()> {
    let meta = SpanMeta::ToolCompleted {
        tool_call_id: "tc_003".into(),
        duration_ms: 10,
        impact: None,
        summary: "ok".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert!(parsed.get("impact").is_none());
    Ok(())
}

#[test]
fn span_meta_tool_failed_json() -> Result<()> {
    let meta = SpanMeta::ToolFailed {
        tool_call_id: "tc_004".into(),
        duration_ms: 100,
        error: "timeout".into(),
        impact: Some(Impact::High),
        summary: "exec cmd".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["error"], "timeout");
    assert_eq!(parsed["impact"], "High");
    Ok(())
}

#[test]
fn span_meta_empty_json() {
    let meta = SpanMeta::Empty {};
    assert_eq!(meta.to_json(), "{}");
}

#[test]
fn span_meta_llm_result_json() -> Result<()> {
    let meta = SpanMeta::LlmResult {
        finish_reason: "stop".into(),
    };
    let parsed: serde_json::Value = serde_json::from_str(&meta.to_json())?;
    assert_eq!(parsed["finish_reason"], "stop");
    Ok(())
}
