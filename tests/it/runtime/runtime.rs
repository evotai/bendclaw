use bendclaw::types::validate_agent_id;

// ── validate_agent_id ──

#[test]
fn validate_simple_alphanumeric() {
    assert!(validate_agent_id("myagent").is_ok());
    assert!(validate_agent_id("Agent123").is_ok());
}

#[test]
fn validate_allows_hyphen_and_underscore() {
    assert!(validate_agent_id("my-agent").is_ok());
    assert!(validate_agent_id("my_agent_v2").is_ok());
}

#[test]
fn validate_rejects_special_chars() {
    assert!(validate_agent_id("my.agent.v2").is_err());
    assert!(validate_agent_id("agent@company").is_err());
    assert!(validate_agent_id("a...b").is_err());
}

#[test]
fn validate_rejects_whitespace() {
    assert!(validate_agent_id("  agent  ").is_err());
    assert!(validate_agent_id("   ").is_err());
}

#[test]
fn validate_empty_rejected() {
    assert!(validate_agent_id("").is_err());
}

#[test]
fn validate_preserves_numbers() {
    assert!(validate_agent_id("agent42").is_ok());
    assert!(validate_agent_id("123").is_ok());
}

// ── sanitize_agent_id ──
