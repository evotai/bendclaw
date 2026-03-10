use anyhow::Result;
use bendclaw::kernel::run::usage::CostSummary;
use bendclaw::kernel::run::usage::ModelRole;
use bendclaw::kernel::run::usage::UsageEvent;
use bendclaw::kernel::run::usage::UsageScope;

#[test]
fn cost_summary_default() {
    let s = CostSummary::default();
    assert_eq!(s.total_tokens, 0);
    assert_eq!(s.total_cost, 0.0);
    assert_eq!(s.record_count, 0);
}

#[test]
fn cost_summary_serde_roundtrip() -> Result<()> {
    let s = CostSummary {
        total_prompt_tokens: 100,
        total_completion_tokens: 50,
        total_reasoning_tokens: 0,
        total_tokens: 150,
        total_cost: 0.05,
        record_count: 3,
        total_cache_read_tokens: 20,
        total_cache_write_tokens: 10,
    };
    let json = serde_json::to_string(&s)?;
    let back: CostSummary = serde_json::from_str(&json)?;
    assert_eq!(back.total_tokens, 150);
    assert_eq!(back.total_cost, 0.05);
    assert_eq!(back.record_count, 3);
    Ok(())
}

#[test]
fn usage_event_clone() {
    let event = UsageEvent {
        agent_id: "a1".into(),
        user_id: "u1".into(),
        session_id: "s1".into(),
        run_id: String::new(),
        provider: "openai".into(),
        model: "gpt-4".into(),
        model_role: ModelRole::Reasoning,
        prompt_tokens: 100,
        completion_tokens: 50,
        reasoning_tokens: 0,
        cache_read_tokens: 10,
        cache_write_tokens: 5,
        ttft_ms: 0,
        cost: 0.01,
    };
    let cloned = event.clone();
    assert_eq!(cloned.agent_id, "a1");
    assert_eq!(cloned.prompt_tokens, 100);
}

#[test]
fn usage_scope_variants() {
    let user = UsageScope::User {
        user_id: "u1".into(),
    };
    let total = UsageScope::AgentTotal {
        agent_id: "a1".into(),
    };
    let daily = UsageScope::AgentDaily {
        agent_id: "a1".into(),
        day: "2026-01-01".into(),
    };
    // Just verify they construct without panic
    let _ = format!("{:?}", user);
    let _ = format!("{:?}", total);
    let _ = format!("{:?}", daily);
}

// ── ModelRole ──

#[test]
fn model_role_as_str() {
    assert_eq!(ModelRole::Reasoning.as_str(), "reasoning");
    assert_eq!(ModelRole::Compaction.as_str(), "compaction");
    assert_eq!(ModelRole::Checkpoint.as_str(), "checkpoint");
}

#[test]
fn model_role_display() {
    assert_eq!(format!("{}", ModelRole::Reasoning), "reasoning");
    assert_eq!(format!("{}", ModelRole::Compaction), "compaction");
    assert_eq!(format!("{}", ModelRole::Checkpoint), "checkpoint");
}

#[test]
fn model_role_default_is_reasoning() {
    assert_eq!(ModelRole::default(), ModelRole::Reasoning);
}

#[test]
fn model_role_serde_roundtrip() -> Result<()> {
    for role in [
        ModelRole::Reasoning,
        ModelRole::Compaction,
        ModelRole::Checkpoint,
    ] {
        let json = serde_json::to_string(&role)?;
        let back: ModelRole = serde_json::from_str(&json)?;
        assert_eq!(back, role);
    }
    Ok(())
}

#[test]
fn cost_summary_all_fields_serialize() -> Result<()> {
    let s = CostSummary {
        total_prompt_tokens: 1000,
        total_completion_tokens: 500,
        total_reasoning_tokens: 200,
        total_tokens: 1700,
        total_cost: 0.15,
        record_count: 10,
        total_cache_read_tokens: 300,
        total_cache_write_tokens: 100,
    };
    let json = serde_json::to_string(&s)?;
    assert!(json.contains("\"total_reasoning_tokens\":200"));
    assert!(json.contains("\"total_cache_read_tokens\":300"));
    assert!(json.contains("\"total_cache_write_tokens\":100"));
    Ok(())
}
