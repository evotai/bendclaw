use std::collections::HashMap;

use bendclaw::storage::AgentConfigRecord;

#[test]
fn agent_config_record_token_limits() {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "".into(),
        display_name: "".into(),
        description: "".into(),
        identity: "".into(),
        soul: "".into(),
        token_limit_total: Some(500000),
        token_limit_daily: Some(50000),
        env: HashMap::new(),
        created_at: "".into(),
        updated_at: "".into(),
    };
    assert_eq!(rec.token_limit_total, Some(500000));
    assert_eq!(rec.token_limit_daily, Some(50000));
}

#[test]
fn agent_config_record_token_limits_none() {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "".into(),
        display_name: "".into(),
        description: "".into(),
        identity: "".into(),
        soul: "".into(),
        token_limit_total: None,
        token_limit_daily: None,
        env: HashMap::new(),
        created_at: "".into(),
        updated_at: "".into(),
    };
    assert!(rec.token_limit_total.is_none());
    assert!(rec.token_limit_daily.is_none());
}

#[test]
fn agent_config_record_serde_roundtrip() {
    let rec = AgentConfigRecord {
        agent_id: "a1".into(),
        system_prompt: "you are helpful".into(),
        display_name: "Agent 1".into(),
        description: "test agent".into(),
        identity: "You are a coding assistant".into(),
        soul: "Be concise and helpful".into(),
        token_limit_total: Some(1_000_000),
        token_limit_daily: None,
        env: HashMap::new(),
        created_at: "2026-01-01".into(),
        updated_at: "2026-01-02".into(),
    };
    let json = serde_json::to_string(&rec).unwrap();
    let back: AgentConfigRecord = serde_json::from_str(&json).unwrap();
    assert_eq!(back.agent_id, "a1");
    assert_eq!(back.system_prompt, "you are helpful");
    assert_eq!(back.identity, "You are a coding assistant");
    assert_eq!(back.soul, "Be concise and helpful");
    assert_eq!(back.token_limit_total, Some(1_000_000));
    assert!(back.token_limit_daily.is_none());
}
