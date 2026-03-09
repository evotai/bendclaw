use bendclaw::storage::databases::AgentDatabases;
use bendclaw::storage::Pool;

#[test]
fn agent_database_name() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bendclaw_").unwrap();
    assert_eq!(dbs.agent_database_name("my-agent"), "bendclaw_my_agent");
}

#[test]
fn prefix_accessor() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "test_").unwrap();
    assert_eq!(dbs.prefix(), "test_");
}

#[test]
fn empty_prefix_rejected() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let result = AgentDatabases::new(pool, "");
    assert!(result.is_err());
}

#[test]
fn invalid_prefix_rejected() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let result = AgentDatabases::new(pool, "bad prefix!");
    assert!(result.is_err());
}

#[test]
fn valid_prefix_with_hyphen() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let result = AgentDatabases::new(pool, "my-prefix");
    assert!(result.is_ok());
}

#[test]
fn agent_pool_creates_scoped_pool() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    let agent_pool = dbs.agent_pool("test-agent").unwrap();
    assert_eq!(agent_pool.base_url(), "https://app.databend.com/v1");
}

// ── agent_database_name sanitization edge cases ──────────────────────────────

#[test]
fn agent_database_name_uppercase_lowered() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("MyAgent"), "bc_myagent");
}

#[test]
fn agent_database_name_consecutive_separators_collapsed() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("a--b..c"), "bc_a_b_c");
}

#[test]
fn agent_database_name_leading_trailing_separators_trimmed() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("--agent--"), "bc_agent");
}

#[test]
fn agent_database_name_empty_agent_id_falls_back_to_default() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name(""), "bc_default");
}

#[test]
fn agent_database_name_all_special_chars_falls_back_to_default() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("---"), "bc_default");
}

#[test]
fn agent_database_name_whitespace_only_falls_back_to_default() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("   "), "bc_default");
}

#[test]
fn agent_database_name_special_chars_replaced() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(
        dbs.agent_database_name("agent@company!v2"),
        "bc_agent_company_v2"
    );
}

#[test]
fn agent_database_name_numeric_agent_id() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("42"), "bc_42");
}

#[test]
fn agent_database_name_with_surrounding_whitespace() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.agent_database_name("  agent  "), "bc_agent");
}

// ── validate_prefix edge cases ───────────────────────────────────────────────

#[test]
fn prefix_with_dot_rejected() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "my.prefix").is_err());
}

#[test]
fn prefix_with_slash_rejected() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "my/prefix").is_err());
}

#[test]
fn prefix_with_unicode_rejected() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "前缀_").is_err());
}

#[test]
fn prefix_all_numeric_accepted() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "123").is_ok());
}

#[test]
fn prefix_single_char_accepted() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "x").is_ok());
}

#[test]
fn prefix_underscores_only_accepted() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    assert!(AgentDatabases::new(pool, "___").is_ok());
}

// ── root_pool accessor ───────────────────────────────────────────────────────

#[test]
fn root_pool_returns_original_pool() {
    let pool = Pool::new("https://app.databend.com", "token", "default").unwrap();
    let dbs = AgentDatabases::new(pool, "bc_").unwrap();
    assert_eq!(dbs.root_pool().base_url(), "https://app.databend.com/v1");
}
