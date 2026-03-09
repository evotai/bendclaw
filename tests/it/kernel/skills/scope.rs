//! Tests for `SkillScope`, `SkillSource`, and `Skill::is_visible_to`.

use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

fn make_skill(scope: SkillScope, agent_id: Option<&str>, user_id: Option<&str>) -> Skill {
    Skill {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        description: "d".to_string(),
        scope,
        source: SkillSource::Agent,
        agent_id: agent_id.map(String::from),
        user_id: user_id.map(String::from),
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: String::new(),
        files: vec![],
        requires: None,
    }
}

// ── SkillScope ────────────────────────────────────────────────────────────────

#[test]
fn scope_as_str_roundtrip() {
    assert_eq!(
        SkillScope::parse(SkillScope::Agent.as_str()),
        SkillScope::Agent
    );
    assert_eq!(
        SkillScope::parse(SkillScope::User.as_str()),
        SkillScope::User
    );
    assert_eq!(
        SkillScope::parse(SkillScope::Global.as_str()),
        SkillScope::Global
    );
}

#[test]
fn scope_parse_unknown_defaults_to_global() {
    assert_eq!(SkillScope::parse("unknown"), SkillScope::Global);
    assert_eq!(SkillScope::parse(""), SkillScope::Global);
}

#[test]
fn scope_display() {
    assert_eq!(format!("{}", SkillScope::Agent), "agent");
    assert_eq!(format!("{}", SkillScope::User), "user");
    assert_eq!(format!("{}", SkillScope::Global), "global");
}

#[test]
fn scope_default_is_global() {
    assert_eq!(SkillScope::default(), SkillScope::Global);
}

#[test]
fn scope_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(&SkillScope::Agent)?;
    assert_eq!(json, "\"agent\"");
    let decoded: SkillScope = serde_json::from_str(&json)?;
    assert_eq!(decoded, SkillScope::Agent);
    Ok(())
}

// ── SkillSource ───────────────────────────────────────────────────────────────

#[test]
fn source_as_str_roundtrip() {
    assert_eq!(
        SkillSource::parse(SkillSource::Local.as_str()),
        SkillSource::Local
    );
    assert_eq!(
        SkillSource::parse(SkillSource::Hub.as_str()),
        SkillSource::Hub
    );
    assert_eq!(
        SkillSource::parse(SkillSource::Github.as_str()),
        SkillSource::Github
    );
    assert_eq!(
        SkillSource::parse(SkillSource::Agent.as_str()),
        SkillSource::Agent
    );
}

#[test]
fn source_parse_unknown_defaults_to_local() {
    assert_eq!(SkillSource::parse("unknown"), SkillSource::Local);
    assert_eq!(SkillSource::parse(""), SkillSource::Local);
}

#[test]
fn source_display() {
    assert_eq!(format!("{}", SkillSource::Local), "local");
    assert_eq!(format!("{}", SkillSource::Hub), "hub");
    assert_eq!(format!("{}", SkillSource::Github), "github");
    assert_eq!(format!("{}", SkillSource::Agent), "agent");
}

#[test]
fn source_default_is_local() {
    assert_eq!(SkillSource::default(), SkillSource::Local);
}

// ── is_visible_to ─────────────────────────────────────────────────────────────

#[test]
fn global_skill_visible_to_everyone() {
    let skill = make_skill(SkillScope::Global, None, None);
    assert!(skill.is_visible_to("agent-1", "user-1"));
    assert!(skill.is_visible_to("agent-2", "user-2"));
    assert!(skill.is_visible_to("any", "any"));
}

#[test]
fn user_skill_visible_to_same_user() {
    let skill = make_skill(SkillScope::User, None, Some("user-1"));
    assert!(skill.is_visible_to("agent-1", "user-1"));
    assert!(skill.is_visible_to("agent-2", "user-1"));
}

#[test]
fn user_skill_not_visible_to_other_user() {
    let skill = make_skill(SkillScope::User, None, Some("user-1"));
    assert!(!skill.is_visible_to("agent-1", "user-2"));
}

#[test]
fn agent_skill_visible_to_same_agent_and_user() {
    let skill = make_skill(SkillScope::Agent, Some("agent-1"), Some("user-1"));
    assert!(skill.is_visible_to("agent-1", "user-1"));
}

#[test]
fn agent_skill_not_visible_to_different_agent_same_user() {
    let skill = make_skill(SkillScope::Agent, Some("agent-1"), Some("user-1"));
    assert!(!skill.is_visible_to("agent-2", "user-1"));
}

#[test]
fn agent_skill_not_visible_to_same_agent_different_user() {
    let skill = make_skill(SkillScope::Agent, Some("agent-1"), Some("user-1"));
    assert!(!skill.is_visible_to("agent-1", "user-2"));
}

#[test]
fn agent_skill_not_visible_to_different_agent_and_user() {
    let skill = make_skill(SkillScope::Agent, Some("agent-1"), Some("user-1"));
    assert!(!skill.is_visible_to("agent-2", "user-2"));
}

// ── Loader sets correct defaults ──────────────────────────────────────────────

#[test]
fn loader_sets_global_scope_and_local_source() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-skill\nversion: 1.0.0\ndescription: d\n---\nbody",
    )?;

    let loaded = bendclaw::kernel::skills::fs::load_skill_from_dir(&skill_dir, "my-skill")
        .ok_or("failed to load skill")?;
    assert_eq!(loaded.skill.scope, SkillScope::Global);
    assert_eq!(loaded.skill.source, SkillSource::Local);
    assert!(loaded.skill.agent_id.is_none());
    assert!(loaded.skill.user_id.is_none());
    Ok(())
}
