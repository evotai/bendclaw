use bendclaw::kernel::sanitize_agent_id;

// ── sanitize_agent_id ──

#[test]
fn sanitize_simple_alphanumeric() {
    assert_eq!(sanitize_agent_id("myagent"), "myagent");
    assert_eq!(sanitize_agent_id("Agent123"), "agent123");
}

#[test]
fn sanitize_replaces_special_chars() {
    assert_eq!(sanitize_agent_id("my-agent"), "my_agent");
    assert_eq!(sanitize_agent_id("my.agent.v2"), "my_agent_v2");
    assert_eq!(sanitize_agent_id("agent@company"), "agent_company");
}

#[test]
fn sanitize_collapses_consecutive_underscores() {
    assert_eq!(sanitize_agent_id("a--b"), "a_b");
    assert_eq!(sanitize_agent_id("a...b"), "a_b");
    assert_eq!(sanitize_agent_id("a-.-b"), "a_b");
}

#[test]
fn sanitize_trims_underscores() {
    assert_eq!(sanitize_agent_id("-agent-"), "agent");
    assert_eq!(sanitize_agent_id("__agent__"), "agent");
    assert_eq!(sanitize_agent_id("  agent  "), "agent");
}

#[test]
fn sanitize_empty_returns_default() {
    assert_eq!(sanitize_agent_id(""), "default");
    assert_eq!(sanitize_agent_id("   "), "default");
    assert_eq!(sanitize_agent_id("---"), "default");
    assert_eq!(sanitize_agent_id("..."), "default");
}

#[test]
fn sanitize_preserves_numbers() {
    assert_eq!(sanitize_agent_id("agent42"), "agent42");
    assert_eq!(sanitize_agent_id("123"), "123");
}

#[test]
fn sanitize_mixed_case_lowered() {
    assert_eq!(sanitize_agent_id("MyAgent"), "myagent");
    assert_eq!(sanitize_agent_id("UPPER"), "upper");
}

// ── sanitize_agent_id ──
