use bendclaw::base::entities::*;

#[test]
fn run_has_all_scope_fields() {
    let run = Run {
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_run_id: String::new(),
        root_trace_id: "t01".into(),
        kind: RunKind::UserTurn.as_str().into(),
        status: RunStatus::Running.as_str().into(),
        input: serde_json::Value::Null,
        output: serde_json::Value::Null,
        error: serde_json::Value::Null,
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(run.user_id, "u01");
    assert_eq!(run.agent_id, "a01");
    assert_eq!(run.session_id, "s01");
    assert_eq!(run.root_trace_id, "t01");
}

#[test]
fn run_event_kind_core_roundtrip() {
    let cases = [
        ("user.input", RunEventKind::UserInput),
        ("assistant.output", RunEventKind::AssistantOutput),
        ("tool.call", RunEventKind::ToolCall),
        ("tool.result", RunEventKind::ToolResult),
        ("skill.enter", RunEventKind::SkillEnter),
        ("skill.exit", RunEventKind::SkillExit),
        ("task.change", RunEventKind::TaskChange),
        ("channel.message", RunEventKind::ChannelMessage),
        ("checkpoint", RunEventKind::Checkpoint),
        ("run.finish", RunEventKind::RunFinish),
        ("run.error", RunEventKind::RunError),
        ("compaction.summary", RunEventKind::CompactionSummary),
    ];
    for (s, expected) in &cases {
        let parsed = RunEventKind::parse(s);
        assert_eq!(&parsed, expected, "parse mismatch for {s}");
        assert_eq!(parsed.as_str(), *s, "as_str mismatch for {s}");
    }
}

#[test]
fn run_event_kind_custom_extensibility() {
    let kind = RunEventKind::parse("mcp.server.connected");
    assert_eq!(kind, RunEventKind::Custom("mcp.server.connected".into()));
    assert_eq!(kind.as_str(), "mcp.server.connected");
}

#[test]
fn run_event_kind_domain_check() {
    assert!(RunEventKind::ToolCall.is_domain("tool"));
    assert!(RunEventKind::ToolResult.is_domain("tool"));
    assert!(!RunEventKind::ToolCall.is_domain("too"));
    assert!(!RunEventKind::UserInput.is_domain("tool"));
    assert!(RunEventKind::SkillEnter.is_domain("skill"));

    let custom = RunEventKind::parse("channel.typing");
    assert!(custom.is_domain("channel"));
    assert!(!custom.is_domain("chan"));
}

#[test]
fn run_event_kind_serde_roundtrip() {
    let kind = RunEventKind::ToolResult;
    let json = serde_json::to_string(&kind).unwrap();
    assert_eq!(json, "\"tool.result\"");
    let back: RunEventKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, RunEventKind::ToolResult);

    let custom = RunEventKind::Custom("my.event".into());
    let json = serde_json::to_string(&custom).unwrap();
    assert_eq!(json, "\"my.event\"");
    let back: RunEventKind = serde_json::from_str(&json).unwrap();
    assert_eq!(back, RunEventKind::Custom("my.event".into()));
}

#[test]
fn run_event_has_all_scope_fields() {
    let evt = RunEvent {
        event_id: "e01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        seq: 1,
        kind: RunEventKind::UserInput,
        payload: serde_json::json!({"text": "hello"}),
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(evt.user_id, "u01");
    assert_eq!(evt.session_id, "s01");
}

#[test]
fn trace_has_parent_trace_id() {
    let trace = Trace {
        trace_id: "t01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_trace_id: "t00".into(),
        name: "root".into(),
        status: "ok".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
        doc: serde_json::json!({"duration_ms": 100}),
    };
    assert_eq!(trace.parent_trace_id, "t00");
}

#[test]
fn span_has_all_ancestor_ids() {
    let span = Span {
        span_id: "sp01".into(),
        trace_id: "t01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_span_id: String::new(),
        name: "llm_call".into(),
        kind: "llm".into(),
        status: "ok".into(),
        created_at: "2026-01-01T00:00:00Z".into(),
        doc: serde_json::json!({"duration_ms": 50, "input_tokens": 100}),
    };
    assert_eq!(span.user_id, "u01");
    assert_eq!(span.agent_id, "a01");
    assert_eq!(span.session_id, "s01");
    assert_eq!(span.run_id, "r01");
    assert_eq!(span.trace_id, "t01");
}

#[test]
fn task_is_agent_scoped() {
    let task = Task {
        task_id: "tk01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        name: "daily report".into(),
        prompt: "generate report".into(),
        enabled: true,
        status: "active".into(),
        schedule: serde_json::Value::Null,
        delivery: serde_json::Value::Null,
        scope: "private".into(),
        created_by: "u01".into(),
        delete_after_run: false,
        run_count: 0,
        last_error: None,
        last_run_at: String::new(),
        next_run_at: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(task.agent_id, "a01");
    assert_eq!(task.user_id, "u01");
}

#[test]
fn task_history_is_agent_scoped() {
    let hist = TaskHistory {
        history_id: "h01".into(),
        task_id: "tk01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        run_id: Some("r01".into()),
        task_name: "daily report".into(),
        schedule: serde_json::Value::Null,
        prompt: "generate report".into(),
        status: "completed".into(),
        output: Some("done".into()),
        error: None,
        duration_ms: Some(1500),
        delivery: serde_json::Value::Null,
        delivery_status: None,
        delivery_error: None,
        executed_by_node_id: None,
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(hist.agent_id, "a01");
    assert_eq!(hist.user_id, "u01");
}

#[test]
fn session_has_agent_scope() {
    let session = Session {
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        title: String::new(),
        scope: String::new(),
        state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(session.agent_id, "a01");
    assert_eq!(session.user_id, "u01");
}

#[test]
fn agent_entity_fields() {
    let agent = Agent {
        agent_id: "a01".into(),
        user_id: "u01".into(),
        name: "test-agent".into(),
        model: "claude-3".into(),
        config: serde_json::Value::Null,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    assert_eq!(agent.user_id, "u01");
}

#[test]
fn run_status_display() {
    assert_eq!(RunStatus::Pending.as_str(), "PENDING");
    assert_eq!(RunStatus::Running.as_str(), "RUNNING");
    assert_eq!(RunStatus::Completed.as_str(), "COMPLETED");
    assert_eq!(RunStatus::Error.as_str(), "ERROR");
    assert_eq!(format!("{}", RunStatus::Cancelled), "CANCELLED");
}

#[test]
fn run_kind_display() {
    assert_eq!(RunKind::UserTurn.as_str(), "user_turn");
    assert_eq!(RunKind::SessionCheckpoint.as_str(), "session_checkpoint");
}

#[test]
fn entities_json_roundtrip() {
    let run = Run {
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_run_id: "r00".into(),
        root_trace_id: "t01".into(),
        kind: "user_turn".into(),
        status: "RUNNING".into(),
        input: serde_json::json!({"prompt": "hello"}),
        output: serde_json::Value::Null,
        error: serde_json::Value::Null,
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&run).unwrap();
    let back: Run = serde_json::from_str(&json).unwrap();
    assert_eq!(back.run_id, "r01");
    assert_eq!(back.parent_run_id, "r00");
    assert_eq!(back.root_trace_id, "t01");
}
