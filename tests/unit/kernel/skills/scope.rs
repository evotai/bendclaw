//! Tests for `SkillScope`, `SkillSource`, and `Skill::is_visible_to`.

use bendclaw::kernel::skills::model::skill::Skill;
use bendclaw::kernel::skills::model::skill::SkillScope;
use bendclaw::kernel::skills::model::skill::SkillSource;

fn make_skill(scope: SkillScope, user_id: &str) -> Skill {
    Skill {
        name: "test".to_string(),
        version: "1.0.0".to_string(),
        description: "d".to_string(),
        scope,
        source: SkillSource::Agent,
        user_id: user_id.to_string(),
        created_by: None,
        last_used_by: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: String::new(),
        files: vec![],
        requires: None,
        manifest: None,
    }
}

// ── SkillScope ────────────────────────────────────────────────────────────────

#[test]
fn scope_as_str_roundtrip() {
    assert_eq!(
        SkillScope::parse(SkillScope::Private.as_str()),
        SkillScope::Private
    );
    assert_eq!(
        SkillScope::parse(SkillScope::Shared.as_str()),
        SkillScope::Shared
    );
}

#[test]
fn scope_parse_legacy_user_maps_to_private() {
    assert_eq!(SkillScope::parse("user"), SkillScope::Private);
}

#[test]
fn scope_parse_unknown_defaults_to_shared() {
    assert_eq!(SkillScope::parse("unknown"), SkillScope::Shared);
    assert_eq!(SkillScope::parse(""), SkillScope::Shared);
}

#[test]
fn scope_display() {
    assert_eq!(format!("{}", SkillScope::Private), "private");
    assert_eq!(format!("{}", SkillScope::Shared), "shared");
}

#[test]
fn scope_default_is_shared() {
    assert_eq!(SkillScope::default(), SkillScope::Shared);
}

#[test]
fn scope_serde_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(&SkillScope::Private)?;
    assert_eq!(json, "\"private\"");
    let decoded: SkillScope = serde_json::from_str(&json)?;
    assert_eq!(decoded, SkillScope::Private);
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
fn shared_skill_visible_to_everyone() {
    let skill = make_skill(SkillScope::Shared, "");
    assert!(skill.is_visible_to("user-1"));
    assert!(skill.is_visible_to("user-2"));
    assert!(skill.is_visible_to("any"));
}

#[test]
fn private_skill_visible_to_same_user() {
    let skill = make_skill(SkillScope::Private, "user-1");
    assert!(skill.is_visible_to("user-1"));
}

#[test]
fn private_skill_not_visible_to_different_user() {
    let skill = make_skill(SkillScope::Private, "user-1");
    assert!(!skill.is_visible_to("user-2"));
}

#[test]
fn private_skill_visible_to_same_user_regardless_of_creator() {
    let skill = make_skill(SkillScope::Private, "user-1");
    assert!(skill.is_visible_to("user-1"));
}

// ── Loader sets correct defaults ──────────────────────────────────────────────

#[test]
fn loader_sets_shared_scope_and_local_source() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let skill_dir = tmp.path().join("my-skill");
    std::fs::create_dir(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: my-skill\nversion: 1.0.0\ndescription: d\n---\nbody",
    )?;

    let loaded = bendclaw::kernel::skills::fs::load_skill_from_dir(&skill_dir, "my-skill")
        .ok_or("failed to load skill")?;
    assert_eq!(loaded.skill.scope, SkillScope::Shared);
    assert_eq!(loaded.skill.source, SkillSource::Local);
    assert!(loaded.skill.created_by.is_none());
    Ok(())
}
