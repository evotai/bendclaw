use anyhow::Result;
use bendclaw::kernel::run::usage::CostSummary;
use bendclaw::storage::AgentConfigRecord;
use bendclaw::storage::SessionRecord;
use bendclaw::storage::UsageRecord;

// ── SessionRecord ──

#[test]
fn session_record_serde_roundtrip() -> Result<()> {
    let record = SessionRecord {
        id: "sess-001".into(),
        agent_id: String::new(),
        user_id: "user-1".into(),
        title: "Test Session".into(),
        scope: String::new(),
        base_key: String::new(),
        replaced_by_session_id: String::new(),
        reset_reason: String::new(),
        session_state: serde_json::Value::Null,
        meta: serde_json::json!({"key": "value"}),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record)?;
    let parsed: SessionRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "sess-001");
    assert_eq!(parsed.user_id, "user-1");
    assert_eq!(parsed.title, "Test Session");
    assert_eq!(
        parsed.meta.get("key").and_then(|v| v.as_str()),
        Some("value")
    );
    Ok(())
}

#[test]
fn session_record_meta_defaults_to_null() -> Result<()> {
    let json =
        r#"{"id":"s","agent_id":"","user_id":"u","title":"t","created_at":"c","updated_at":"u2"}"#;
    let parsed: SessionRecord = serde_json::from_str(json)?;
    assert!(parsed.meta.is_null());
    Ok(())
}

// ── AgentConfigRecord ──

#[test]
fn agent_config_record_serde_roundtrip() -> Result<()> {
    let record = AgentConfigRecord {
        agent_id: "agent-1".into(),
        system_prompt: "You are helpful".into(),
        display_name: "TestBot".into(),
        description: "A test agent".into(),
        identity: String::new(),
        soul: String::new(),
        token_limit_total: None,
        token_limit_daily: None,
        llm_config: None,
        created_by: String::new(),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record)?;
    let parsed: AgentConfigRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.agent_id, "agent-1");
    assert_eq!(parsed.system_prompt, "You are helpful");
    assert_eq!(parsed.display_name, "TestBot");
    assert_eq!(parsed.description, "A test agent");
    Ok(())
}

// ── UsageRecord ──

#[test]
fn usage_record_serde_roundtrip() -> Result<()> {
    let record = UsageRecord {
        id: "usage-001".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        session_id: "sess-1".into(),
        run_id: String::new(),
        provider: "openai".into(),
        model: "gpt-4".into(),
        model_role: String::new(),
        prompt_tokens: 1000,
        completion_tokens: 500,
        reasoning_tokens: 0,
        total_tokens: 1500,
        cache_read_tokens: 200,
        cache_write_tokens: 100,
        ttft_ms: 0,
        cost: 0.045,
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&record)?;
    let parsed: UsageRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "usage-001");
    assert_eq!(parsed.provider, "openai");
    assert_eq!(parsed.model, "gpt-4");
    assert_eq!(parsed.prompt_tokens, 1000);
    assert_eq!(parsed.completion_tokens, 500);
    assert!((parsed.cost - 0.045).abs() < f64::EPSILON);
    Ok(())
}

// ── RunStatus ──

#[test]
fn run_status_as_str() {
    use bendclaw::storage::RunStatus;
    assert_eq!(RunStatus::Pending.as_str(), "PENDING");
    assert_eq!(RunStatus::Running.as_str(), "RUNNING");
    assert_eq!(RunStatus::Paused.as_str(), "PAUSED");
    assert_eq!(RunStatus::Completed.as_str(), "COMPLETED");
    assert_eq!(RunStatus::Error.as_str(), "ERROR");
    assert_eq!(RunStatus::Cancelled.as_str(), "CANCELLED");
}

#[test]
fn run_status_display() {
    use bendclaw::storage::RunStatus;
    assert_eq!(RunStatus::Pending.to_string(), "PENDING");
    assert_eq!(RunStatus::Running.to_string(), "RUNNING");
    assert_eq!(RunStatus::Paused.to_string(), "PAUSED");
    assert_eq!(RunStatus::Completed.to_string(), "COMPLETED");
    assert_eq!(RunStatus::Error.to_string(), "ERROR");
    assert_eq!(RunStatus::Cancelled.to_string(), "CANCELLED");
}

#[test]
fn run_status_serde_roundtrip() -> anyhow::Result<()> {
    use bendclaw::storage::RunStatus;
    for status in [
        RunStatus::Pending,
        RunStatus::Running,
        RunStatus::Paused,
        RunStatus::Completed,
        RunStatus::Error,
        RunStatus::Cancelled,
    ] {
        let json = serde_json::to_string(&status)?;
        let parsed: RunStatus = serde_json::from_str(&json)?;
        assert_eq!(parsed, status);
    }
    Ok(())
}

#[test]
fn run_status_serde_string_values() -> anyhow::Result<()> {
    use bendclaw::storage::RunStatus;
    assert_eq!(serde_json::to_string(&RunStatus::Pending)?, r#""PENDING""#);
    assert_eq!(serde_json::to_string(&RunStatus::Running)?, r#""RUNNING""#);
    assert_eq!(serde_json::to_string(&RunStatus::Paused)?, r#""PAUSED""#);
    assert_eq!(
        serde_json::to_string(&RunStatus::Completed)?,
        r#""COMPLETED""#
    );
    assert_eq!(serde_json::to_string(&RunStatus::Error)?, r#""ERROR""#);
    assert_eq!(
        serde_json::to_string(&RunStatus::Cancelled)?,
        r#""CANCELLED""#
    );
    Ok(())
}

// ── RunMetrics ──

#[test]
fn run_metrics_default() {
    use bendclaw::storage::RunMetrics;
    let m = RunMetrics::default();
    assert_eq!(m.prompt_tokens, 0);
    assert_eq!(m.completion_tokens, 0);
    assert_eq!(m.reasoning_tokens, 0);
    assert_eq!(m.total_tokens, 0);
    assert_eq!(m.cache_read_tokens, 0);
    assert_eq!(m.cache_write_tokens, 0);
    assert_eq!(m.ttft_ms, 0);
    assert_eq!(m.duration_ms, 0);
    assert_eq!(m.cost, 0.0);
}

#[test]
fn run_metrics_serde_roundtrip() -> anyhow::Result<()> {
    use bendclaw::storage::RunMetrics;
    let m = RunMetrics {
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 10,
        total_tokens: 160,
        cache_read_tokens: 20,
        cache_write_tokens: 5,
        ttft_ms: 300,
        duration_ms: 1200,
        cost: 0.0042,
    };
    let json = serde_json::to_string(&m)?;
    let parsed: RunMetrics = serde_json::from_str(&json)?;
    assert_eq!(parsed.prompt_tokens, 100);
    assert_eq!(parsed.completion_tokens, 50);
    assert_eq!(parsed.reasoning_tokens, 10);
    assert_eq!(parsed.total_tokens, 160);
    assert_eq!(parsed.cache_read_tokens, 20);
    assert_eq!(parsed.cache_write_tokens, 5);
    assert_eq!(parsed.ttft_ms, 300);
    assert_eq!(parsed.duration_ms, 1200);
    assert!((parsed.cost - 0.0042).abs() < f64::EPSILON);
    Ok(())
}

// ── RunRecord ──

fn make_run_record(metrics_json: &str) -> bendclaw::storage::RunRecord {
    bendclaw::storage::RunRecord {
        id: "run-001".into(),
        session_id: "sess-1".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        kind: "user_turn".into(),
        parent_run_id: String::new(),
        node_id: String::new(),
        status: "COMPLETED".into(),
        input: "hello".into(),
        output: "world".into(),
        error: String::new(),
        metrics: metrics_json.into(),
        stop_reason: "end_turn".into(),
        checkpoint_through_run_id: String::new(),
        iterations: 3,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    }
}

#[test]
fn run_record_serde_roundtrip() -> anyhow::Result<()> {
    let record = make_run_record("{}");
    let json = serde_json::to_string(&record)?;
    let parsed: bendclaw::storage::RunRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "run-001");
    assert_eq!(parsed.session_id, "sess-1");
    assert_eq!(parsed.agent_id, "agent-1");
    assert_eq!(parsed.kind, "user_turn");
    assert_eq!(parsed.status, "COMPLETED");
    assert_eq!(parsed.iterations, 3);
    Ok(())
}

#[test]
fn run_record_parse_metrics_empty_string() -> anyhow::Result<()> {
    let record = make_run_record("");
    let m = record.parse_metrics()?;
    assert_eq!(m.prompt_tokens, 0);
    assert_eq!(m.total_tokens, 0);
    assert_eq!(m.cost, 0.0);
    Ok(())
}

#[test]
fn run_record_parse_metrics_invalid_json() {
    let record = make_run_record("not-json");
    assert!(record.parse_metrics().is_err());
}

#[test]
fn run_record_parse_metrics_valid_json() -> anyhow::Result<()> {
    use bendclaw::storage::RunMetrics;
    let metrics = RunMetrics {
        prompt_tokens: 200,
        completion_tokens: 80,
        total_tokens: 280,
        cost: 0.01,
        ..Default::default()
    };
    let json = serde_json::to_string(&metrics)?;
    let record = make_run_record(&json);
    let parsed = record.parse_metrics()?;
    assert_eq!(parsed.prompt_tokens, 200);
    assert_eq!(parsed.completion_tokens, 80);
    assert_eq!(parsed.total_tokens, 280);
    assert!((parsed.cost - 0.01).abs() < f64::EPSILON);
    Ok(())
}

// ── CostSummary ──

#[test]
fn cost_summary_default() {
    let s = CostSummary::default();
    assert_eq!(s.total_prompt_tokens, 0);
    assert_eq!(s.total_completion_tokens, 0);
    assert_eq!(s.total_tokens, 0);
    assert_eq!(s.total_cost, 0.0);
    assert_eq!(s.record_count, 0);
}

#[test]
fn cost_summary_serde_roundtrip() -> Result<()> {
    let s = CostSummary {
        total_prompt_tokens: 1000,
        total_completion_tokens: 500,
        total_reasoning_tokens: 0,
        total_tokens: 1500,
        total_cost: 0.05,
        record_count: 10,
        total_cache_read_tokens: 200,
        total_cache_write_tokens: 100,
    };
    let json = serde_json::to_string(&s)?;
    let parsed: CostSummary = serde_json::from_str(&json)?;
    assert_eq!(parsed.total_prompt_tokens, 1000);
    assert_eq!(parsed.total_completion_tokens, 500);
    assert_eq!(parsed.total_tokens, 1500);
    assert!((parsed.total_cost - 0.05).abs() < f64::EPSILON);
    assert_eq!(parsed.record_count, 10);
    assert_eq!(parsed.total_cache_read_tokens, 200);
    assert_eq!(parsed.total_cache_write_tokens, 100);
    Ok(())
}

// ── RunRecord::metrics_json ──

#[test]
fn run_record_metrics_json_empty_string_returns_null() -> anyhow::Result<()> {
    let record = make_run_record("");
    let v = record.metrics_json()?;
    assert!(v.is_null());
    Ok(())
}

#[test]
fn run_record_metrics_json_invalid_returns_error() {
    let record = make_run_record("not-json");
    assert!(record.metrics_json().is_err());
}

#[test]
fn run_record_metrics_json_valid_returns_object() -> anyhow::Result<()> {
    let record = make_run_record(r#"{"prompt_tokens":10,"total_tokens":15}"#);
    let v = record.metrics_json()?;
    assert!(v.is_object());
    assert_eq!(v["prompt_tokens"], 10);
    assert_eq!(v["total_tokens"], 15);
    Ok(())
}

// ── RunEventRecord ──

#[test]
fn run_event_record_payload_json_valid() -> anyhow::Result<()> {
    use bendclaw::storage::RunEventRecord;
    let rec = RunEventRecord {
        id: "ev-001".into(),
        run_id: "run-1".into(),
        session_id: "sess-1".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        seq: 1,
        event: "llm.request".into(),
        payload: r#"{"model":"gpt-4","tokens":100}"#.into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let v = rec.payload_json()?;
    assert_eq!(v["model"], "gpt-4");
    assert_eq!(v["tokens"], 100);
    Ok(())
}

#[test]
fn run_event_record_payload_json_invalid_returns_error() {
    use bendclaw::storage::RunEventRecord;
    let rec = RunEventRecord {
        id: "ev-002".into(),
        run_id: "run-1".into(),
        session_id: "sess-1".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        seq: 2,
        event: "bad".into(),
        payload: "not-json".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    assert!(rec.payload_json().is_err());
}

#[test]
fn run_event_record_serde_roundtrip() -> anyhow::Result<()> {
    use bendclaw::storage::RunEventRecord;
    let rec = RunEventRecord {
        id: "ev-003".into(),
        run_id: "run-2".into(),
        session_id: "sess-2".into(),
        agent_id: "agent-2".into(),
        user_id: "user-2".into(),
        seq: 5,
        event: "tool.completed".into(),
        payload: r#"{"tool":"shell"}"#.into(),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&rec)?;
    let back: RunEventRecord = serde_json::from_str(&json)?;
    assert_eq!(back.id, "ev-003");
    assert_eq!(back.seq, 5);
    assert_eq!(back.event, "tool.completed");
    Ok(())
}
