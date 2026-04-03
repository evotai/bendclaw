use bendclaw::execution::default_identity::default_identity;

#[test]
fn default_identity_is_not_empty() {
    assert!(!default_identity().is_empty());
}

#[test]
fn default_identity_contains_agent_name() {
    assert!(default_identity().contains("BendClaw"));
}
