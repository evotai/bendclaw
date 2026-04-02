use bendclaw::app::result::run_event_mapper::map_run_event;
use bendclaw::types::entities::RunEvent;
use bendclaw::types::entities::RunEventKind;

#[test]
fn maps_run_event_to_envelope() {
    let event = RunEvent {
        event_id: "e01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        seq: 5,
        kind: RunEventKind::ToolResult,
        payload: serde_json::json!({"output": "done"}),
        created_at: "2026-01-01T00:00:00Z".into(),
    };

    let envelope = map_run_event(&event);
    assert_eq!(envelope.sequence, 5);
    assert_eq!(envelope.event_name, "tool.result");
    assert_eq!(envelope.session_id, "s01");
    assert_eq!(envelope.run_id, "r01");
    assert_eq!(envelope.payload["output"], "done");
}

#[test]
fn maps_custom_event_kind() {
    let event = RunEvent {
        event_id: "e02".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        seq: 1,
        kind: RunEventKind::Custom("mcp.connected".into()),
        payload: serde_json::json!({}),
        created_at: "2026-01-01T00:00:00Z".into(),
    };

    let envelope = map_run_event(&event);
    assert_eq!(envelope.event_name, "mcp.connected");
}
