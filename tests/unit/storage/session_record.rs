use anyhow::Result;
use bendclaw::storage::SessionRecord;

#[test]
fn session_record_serde_roundtrip() -> Result<()> {
    let rec = SessionRecord {
        id: "s1".into(),
        agent_id: String::new(),
        user_id: "u1".into(),
        title: "My session".into(),
        session_state: serde_json::Value::Null,
        meta: serde_json::json!({"key": "value"}),
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-02".into(),
    };
    let json = serde_json::to_string(&rec)?;
    let back: SessionRecord = serde_json::from_str(&json)?;
    assert_eq!(back.id, "s1");
    assert_eq!(back.title, "My session");
    assert_eq!(back.meta["key"], "value");
    Ok(())
}
