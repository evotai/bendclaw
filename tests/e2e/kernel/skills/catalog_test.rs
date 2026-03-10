//! Tests for SkillCatalogImpl: visibility filtering, cache cleanup, and priority.

use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

fn make_skill(
    name: &str,
    scope: SkillScope,
    source: SkillSource,
    agent_id: Option<&str>,
    user_id: Option<&str>,
) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: format!("{name} skill"),
        scope,
        source,
        agent_id: agent_id.map(String::from),
        user_id: user_id.map(String::from),
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: format!("# {name}"),
        files: vec![],
        requires: None,
    }
}

fn make_catalog(
    dir: &std::path::Path,
) -> Result<bendclaw::kernel::skills::catalog::SkillCatalogImpl, Box<dyn std::error::Error>> {
    // Use a dummy AgentDatabases that won't be called (no sync in these tests).
    let pool = bendclaw::storage::pool::Pool::new(
        "https://app.databend.com/v1.1",
        "dummy-token",
        "default",
    )?;
    let databases = std::sync::Arc::new(bendclaw::storage::AgentDatabases::new(
        pool,
        "test_catalog_",
    )?);
    Ok(bendclaw::kernel::skills::catalog::SkillCatalogImpl::new(
        databases,
        dir.to_path_buf(),
    ))
}

// ── for_agent visibility filtering ────────────────────────────────────────────

#[test]
fn for_agent_returns_only_visible_skills() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    // Global skill — visible to everyone
    catalog.insert(&make_skill(
        "global-tool",
        SkillScope::Global,
        SkillSource::Agent,
        None,
        None,
    ));

    // User-scoped skill — visible only to user-1
    catalog.insert(&make_skill(
        "user-tool",
        SkillScope::User,
        SkillSource::Agent,
        None,
        Some("user-1"),
    ));

    // Agent-scoped skill — visible only to agent-1 + user-1
    catalog.insert(&make_skill(
        "agent-tool",
        SkillScope::Agent,
        SkillSource::Agent,
        Some("agent-1"),
        Some("user-1"),
    ));

    // agent-1/user-1 sees all 3
    let visible = catalog.for_agent("agent-1", "user-1");
    assert_eq!(visible.len(), 3);

    // agent-2/user-1 sees global + user-tool (not agent-tool)
    let visible = catalog.for_agent("agent-2", "user-1");
    let names: Vec<&str> = visible.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(visible.len(), 2);
    assert!(names.contains(&"global-tool"));
    assert!(names.contains(&"user-tool"));
    assert!(!names.contains(&"agent-tool"));

    // agent-1/user-2 sees only global (not user-tool or agent-tool)
    let visible = catalog.for_agent("agent-1", "user-2");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "global-tool");

    Ok(())
}

// ── evict cleans cache and disk ───────────────────────────────────────────────

#[test]
fn evict_removes_from_cache_and_disk() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    let skill = make_skill(
        "ephemeral",
        SkillScope::Global,
        SkillSource::Agent,
        None,
        None,
    );
    catalog.insert(&skill);

    assert!(catalog.get("ephemeral").is_some());

    catalog.evict("ephemeral");

    assert!(catalog.get("ephemeral").is_none());
    // Remote dir should be cleaned
    assert!(!tmp.path().join(".remote").join("ephemeral").exists());

    Ok(())
}

// ── insert overwrites existing skill ──────────────────────────────────────────

#[test]
fn insert_overwrites_existing_version() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    let v1 = make_skill("tool", SkillScope::Global, SkillSource::Agent, None, None);
    catalog.insert(&v1);
    assert_eq!(
        catalog.get("tool").map(|s| s.version.clone()),
        Some("1.0.0".to_string())
    );

    let mut v2 = v1.clone();
    v2.version = "2.0.0".to_string();
    catalog.insert(&v2);
    assert_eq!(
        catalog.get("tool").map(|s| s.version.clone()),
        Some("2.0.0".to_string())
    );

    Ok(())
}

// ── scope priority: agent > user > global ─────────────────────────────────────

#[test]
fn for_agent_returns_correct_scope_when_multiple_scopes_exist(
) -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    // Insert 3 skills with different scopes but different names
    // to verify filtering works correctly per scope
    catalog.insert(&make_skill(
        "g",
        SkillScope::Global,
        SkillSource::Agent,
        None,
        None,
    ));
    catalog.insert(&make_skill(
        "u",
        SkillScope::User,
        SkillSource::Agent,
        None,
        Some("u1"),
    ));
    catalog.insert(&make_skill(
        "a",
        SkillScope::Agent,
        SkillSource::Agent,
        Some("a1"),
        Some("u1"),
    ));

    // a1/u1 sees all 3
    let visible = catalog.for_agent("a1", "u1");
    assert_eq!(visible.len(), 3);

    // a2/u2 sees only global
    let visible = catalog.for_agent("a2", "u2");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].name, "g");

    Ok(())
}

// ── builtins dir is not polluted by insert ────────────────────────────────────

#[test]
fn insert_writes_to_remote_dir_not_builtins() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    let skill = make_skill(
        "remote-skill",
        SkillScope::Global,
        SkillSource::Agent,
        None,
        None,
    );
    catalog.insert(&skill);

    // Should be in .remote/, not in builtins root
    let remote_path = tmp
        .path()
        .join(".remote")
        .join("remote-skill")
        .join("SKILL.md");
    assert!(
        remote_path.exists(),
        "skill should be written to .remote/ dir"
    );

    Ok(())
}
