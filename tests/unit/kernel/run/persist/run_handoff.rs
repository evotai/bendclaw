use bendclaw::kernel::run::persist::run_handoff::RunHandoff;

#[test]
fn handoff_serde_roundtrip() {
    let handoff = RunHandoff {
        run_id: "r01".into(),
        session_id: "s01".into(),
        last_turn: 5,
        pending_tool_calls: vec!["tc01".into()],
        compaction_checkpoint: Some(serde_json::json!({"through_run_id": "r00"})),
        partial_output: "partial text".into(),
    };
    let json = handoff.to_json().unwrap();
    let back = RunHandoff::from_json(&json).unwrap();
    assert_eq!(back.run_id, "r01");
    assert_eq!(back.last_turn, 5);
    assert_eq!(back.pending_tool_calls.len(), 1);
    assert!(!back.partial_output.is_empty());
}

#[test]
fn handoff_default_is_empty() {
    let handoff = RunHandoff::default();
    assert_eq!(handoff.last_turn, 0);
    assert!(handoff.pending_tool_calls.is_empty());
    assert!(handoff.compaction_checkpoint.is_none());
}
