use anyhow::Result;
use bendclaw::execution::Event;
use bendclaw::kernel::tools::operation::OpType;
use bendclaw::kernel::tools::operation::OperationMeta;
use bendclaw::kernel::workbench::replay::project_replay;
use bendclaw::kernel::workbench::replay::ReplayFacts;
use bendclaw::kernel::workbench::sem_event::SemEvent;
use bendclaw::storage::dal::run::record::RunRecord;
use bendclaw::storage::dal::run_event::record::RunEventRecord;

fn make_run(id: &str, status: &str, stop_reason: &str, error: &str) -> RunRecord {
    RunRecord {
        id: id.to_string(),
        session_id: "sess_1".to_string(),
        agent_id: "agent_1".to_string(),
        user_id: "user_1".to_string(),
        kind: "user_turn".to_string(),
        parent_run_id: String::new(),
        node_id: String::new(),
        status: status.to_string(),
        input: String::new(),
        output: String::new(),
        error: error.to_string(),
        metrics: serde_json::to_string(&bendclaw::storage::dal::run::record::RunMetrics {
            duration_ms: 1500,
            ..Default::default()
        })
        .unwrap_or_default(),
        stop_reason: stop_reason.to_string(),
        checkpoint_through_run_id: String::new(),
        iterations: 2,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn make_event_record(run_id: &str, seq: u32, event: &Event) -> RunEventRecord {
    RunEventRecord {
        id: format!("evt_{seq}"),
        run_id: run_id.to_string(),
        session_id: "sess_1".to_string(),
        agent_id: "agent_1".to_string(),
        user_id: "user_1".to_string(),
        seq,
        event: event.name(),
        payload: serde_json::to_string(event).unwrap_or_default(),
        created_at: String::new(),
    }
}

#[test]
fn project_replay_empty_facts() {
    let facts = ReplayFacts {
        runs: vec![],
        events: vec![],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.session_id, "sess_1");
    assert!(summary.runs.is_empty());
    assert!(summary.tool_timeline.is_empty());
    assert!(summary.capabilities_by_run.is_empty());
    assert_eq!(summary.outcome.final_status, "");
}

#[test]
fn project_replay_reverses_runs_to_chronological() {
    // Storage returns DESC — run_2 first, run_1 second.
    let facts = ReplayFacts {
        runs: vec![
            make_run("run_2", "COMPLETED", "end_turn", ""),
            make_run("run_1", "COMPLETED", "end_turn", ""),
        ],
        events: vec![],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.runs.len(), 2);
    assert_eq!(summary.runs[0].run_id, "run_1");
    assert_eq!(summary.runs[1].run_id, "run_2");
}

#[test]
fn project_replay_outcome_from_last_run() {
    let facts = ReplayFacts {
        runs: vec![
            make_run("run_2", "ERROR", "error", "something broke"),
            make_run("run_1", "COMPLETED", "end_turn", ""),
        ],
        events: vec![],
    };
    let summary = project_replay("sess_1", facts);
    // After reverse, run_2 is last (chronologically latest).
    assert_eq!(summary.outcome.final_status, "ERROR");
    assert_eq!(summary.outcome.final_stop_reason, "error");
    assert_eq!(summary.outcome.error.as_deref(), Some("something broke"));
}

#[test]
fn project_replay_tool_timeline() -> Result<()> {
    let tool_start = Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        arguments: serde_json::json!({"command": "ls"}),
    };
    let mut op = OperationMeta::new(OpType::Execute);
    op.duration_ms = 42;
    let tool_end = Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        success: true,
        output: "file.txt".into(),
        operation: op,
    };
    let facts = ReplayFacts {
        runs: vec![make_run("run_1", "COMPLETED", "end_turn", "")],
        events: vec![
            make_event_record("run_1", 1, &tool_start),
            make_event_record("run_1", 2, &tool_end),
        ],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.tool_timeline.len(), 1);
    let entry = &summary.tool_timeline[0];
    assert_eq!(entry.tool_call_id, "tc_1");
    assert_eq!(entry.name, "shell");
    assert!(entry.success);
    assert_eq!(entry.duration_ms, Some(42));
    assert_eq!(entry.run_id, "run_1");
    Ok(())
}

#[test]
fn project_replay_capabilities_from_semantic_event() {
    let sem = Event::Semantic(SemEvent::CapabilitiesSnapshot {
        tools: vec!["shell".into(), "grep".into()],
        skills: vec!["commit".into()],
    });
    let facts = ReplayFacts {
        runs: vec![make_run("run_1", "COMPLETED", "end_turn", "")],
        events: vec![make_event_record("run_1", 1, &sem)],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.capabilities_by_run.len(), 1);
    let cap = &summary.capabilities_by_run[0];
    assert_eq!(cap.run_id, "run_1");
    assert_eq!(cap.tools, vec!["shell", "grep"]);
    assert_eq!(cap.skills, vec!["commit"]);
}

#[test]
fn project_replay_skips_stream_delta() {
    let delta = Event::StreamDelta(bendclaw::execution::Delta::Text {
        content: "hello".into(),
    });
    let facts = ReplayFacts {
        runs: vec![make_run("run_1", "COMPLETED", "end_turn", "")],
        events: vec![make_event_record("run_1", 1, &delta)],
    };
    let summary = project_replay("sess_1", facts);
    assert!(summary.tool_timeline.is_empty());
    assert!(summary.capabilities_by_run.is_empty());
}

#[test]
fn project_replay_graceful_on_bad_payload() {
    let bad_record = RunEventRecord {
        id: "evt_1".to_string(),
        run_id: "run_1".to_string(),
        session_id: "sess_1".to_string(),
        agent_id: "agent_1".to_string(),
        user_id: "user_1".to_string(),
        seq: 1,
        event: "ToolStart".to_string(),
        payload: "not valid json".to_string(),
        created_at: String::new(),
    };
    let facts = ReplayFacts {
        runs: vec![make_run("run_1", "COMPLETED", "end_turn", "")],
        events: vec![bad_record],
    };
    // Should not panic — bad payloads are skipped.
    let summary = project_replay("sess_1", facts);
    assert!(summary.tool_timeline.is_empty());
}

#[test]
fn project_replay_run_summary_fields() {
    let facts = ReplayFacts {
        runs: vec![make_run("run_1", "COMPLETED", "end_turn", "")],
        events: vec![],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.runs.len(), 1);
    let run = &summary.runs[0];
    assert_eq!(run.run_id, "run_1");
    assert_eq!(run.status, "COMPLETED");
    assert_eq!(run.stop_reason, "end_turn");
    assert_eq!(run.iterations, 2);
    assert_eq!(run.duration_ms, 1500);
    assert_eq!(run.error, "");
}

#[test]
fn project_replay_multi_run_tool_timeline_order() {
    // Two runs, each with a tool call. Events arrive in created_at order (run_1 first).
    let ts1 = Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        arguments: serde_json::json!({}),
    };
    let mut op1 = OperationMeta::new(OpType::Execute);
    op1.duration_ms = 10;
    let te1 = Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        success: true,
        output: "ok".into(),
        operation: op1,
    };
    let ts2 = Event::ToolStart {
        tool_call_id: "tc_2".into(),
        name: "grep".into(),
        arguments: serde_json::json!({}),
    };
    let mut op2 = OperationMeta::new(OpType::Execute);
    op2.duration_ms = 20;
    let te2 = Event::ToolEnd {
        tool_call_id: "tc_2".into(),
        name: "grep".into(),
        success: false,
        output: "err".into(),
        operation: op2,
    };

    let facts = ReplayFacts {
        runs: vec![
            make_run("run_2", "ERROR", "error", "grep failed"),
            make_run("run_1", "COMPLETED", "end_turn", ""),
        ],
        // Events in created_at order: run_1 events first, then run_2.
        events: vec![
            make_event_record("run_1", 1, &ts1),
            make_event_record("run_1", 2, &te1),
            make_event_record("run_2", 1, &ts2),
            make_event_record("run_2", 2, &te2),
        ],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.tool_timeline.len(), 2);
    assert_eq!(summary.tool_timeline[0].run_id, "run_1");
    assert_eq!(summary.tool_timeline[0].name, "shell");
    assert!(summary.tool_timeline[0].success);
    assert_eq!(summary.tool_timeline[1].run_id, "run_2");
    assert_eq!(summary.tool_timeline[1].name, "grep");
    assert!(!summary.tool_timeline[1].success);
}

#[test]
fn project_replay_cross_run_duplicate_tool_call_id() {
    // Two runs with the same tool_call_id — ToolEnd must match within its own run.
    let ts1 = Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        arguments: serde_json::json!({}),
    };
    let mut op1 = OperationMeta::new(OpType::Execute);
    op1.duration_ms = 10;
    let te1 = Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        success: true,
        output: "ok".into(),
        operation: op1,
    };
    let ts2 = Event::ToolStart {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        arguments: serde_json::json!({}),
    };
    let mut op2 = OperationMeta::new(OpType::Execute);
    op2.duration_ms = 99;
    let te2 = Event::ToolEnd {
        tool_call_id: "tc_1".into(),
        name: "shell".into(),
        success: false,
        output: "fail".into(),
        operation: op2,
    };

    let facts = ReplayFacts {
        runs: vec![
            make_run("run_2", "ERROR", "error", "fail"),
            make_run("run_1", "COMPLETED", "end_turn", ""),
        ],
        events: vec![
            make_event_record("run_1", 1, &ts1),
            make_event_record("run_1", 2, &te1),
            make_event_record("run_2", 1, &ts2),
            make_event_record("run_2", 2, &te2),
        ],
    };
    let summary = project_replay("sess_1", facts);
    assert_eq!(summary.tool_timeline.len(), 2);
    // run_1's entry: success=true, duration=10
    assert_eq!(summary.tool_timeline[0].run_id, "run_1");
    assert!(summary.tool_timeline[0].success);
    assert_eq!(summary.tool_timeline[0].duration_ms, Some(10));
    // run_2's entry: success=false, duration=99 — NOT contaminated by run_1
    assert_eq!(summary.tool_timeline[1].run_id, "run_2");
    assert!(!summary.tool_timeline[1].success);
    assert_eq!(summary.tool_timeline[1].duration_ms, Some(99));
}
