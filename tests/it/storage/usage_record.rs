use anyhow::Result;
use bendclaw::storage::UsageRecord;

#[test]
fn usage_record_serde_roundtrip() -> Result<()> {
    let rec = UsageRecord {
        id: "u1".into(),
        agent_id: "a1".into(),
        user_id: "user1".into(),
        session_id: "s1".into(),
        run_id: String::new(),
        provider: "openai".into(),
        model: "gpt-4".into(),
        model_role: String::new(),
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 0,
        total_tokens: 150,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
        ttft_ms: 0,
        cost: 0.05,
        created_at: "2026-01-01T00:00:00Z".into(),
    };
    let json = serde_json::to_string(&rec)?;
    let back: UsageRecord = serde_json::from_str(&json)?;
    assert_eq!(back.id, "u1");
    assert_eq!(back.total_tokens, 150);
    assert_eq!(back.cost, 0.05);
    Ok(())
}
