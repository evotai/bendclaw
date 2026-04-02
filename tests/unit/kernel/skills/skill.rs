use anyhow::Result;
use bendclaw::kernel::skills::model::skill::Skill;
use bendclaw::kernel::skills::model::skill::SkillFile;
use bendclaw::kernel::skills::model::skill::SkillParameter;
use bendclaw::kernel::skills::model::skill::SkillRequirements;
use bendclaw::kernel::skills::model::skill::SkillScope;
use bendclaw::kernel::skills::model::skill::SkillSource;

#[test]
fn skill_scope_display() {
    assert_eq!(SkillScope::Private.to_string(), "private");
    assert_eq!(SkillScope::Shared.to_string(), "shared");
}

#[test]
fn skill_scope_as_str() {
    assert_eq!(SkillScope::Private.as_str(), "private");
    assert_eq!(SkillScope::Shared.as_str(), "shared");
}

#[test]
fn skill_scope_parse() {
    assert_eq!(SkillScope::parse("private"), SkillScope::Private);
    assert_eq!(SkillScope::parse("agent"), SkillScope::Private); // legacy mapping
    assert_eq!(SkillScope::parse("user"), SkillScope::Private); // legacy mapping
    assert_eq!(SkillScope::parse("shared"), SkillScope::Shared);
    assert_eq!(SkillScope::parse("unknown"), SkillScope::Shared);
}

#[test]
fn skill_scope_default_is_shared() {
    assert_eq!(SkillScope::default(), SkillScope::Shared);
}

#[test]
fn skill_scope_serde_roundtrip() -> Result<()> {
    for scope in [SkillScope::Private, SkillScope::Shared] {
        let json = serde_json::to_string(&scope)?;
        let back: SkillScope = serde_json::from_str(&json)?;
        assert_eq!(back, scope);
    }
    Ok(())
}

#[test]
fn skill_source_display() {
    assert_eq!(SkillSource::Local.to_string(), "local");
    assert_eq!(SkillSource::Hub.to_string(), "hub");
    assert_eq!(SkillSource::Github.to_string(), "github");
    assert_eq!(SkillSource::Agent.to_string(), "agent");
}

#[test]
fn skill_source_parse() {
    assert_eq!(SkillSource::parse("local"), SkillSource::Local);
    assert_eq!(SkillSource::parse("hub"), SkillSource::Hub);
    assert_eq!(SkillSource::parse("github"), SkillSource::Github);
    assert_eq!(SkillSource::parse("agent"), SkillSource::Agent);
    assert_eq!(SkillSource::parse("unknown"), SkillSource::Local);
}

#[test]
fn skill_source_default_is_local() {
    assert_eq!(SkillSource::default(), SkillSource::Local);
}

fn test_skill(scope: SkillScope, user_id: &str) -> Skill {
    Skill {
        name: "test".into(),
        version: "0.1.0".into(),
        description: "test skill".into(),
        scope,
        source: SkillSource::Local,
        user_id: user_id.to_string(),
        created_by: None,
        last_used_by: None,
        timeout: 30,
        executable: true,
        parameters: vec![],
        content: "print('hello')".into(),
        files: vec![],
        requires: None,
        manifest: None,
    }
}

#[test]
fn shared_skill_visible_to_anyone() {
    let skill = test_skill(SkillScope::Shared, "");
    assert!(skill.is_visible_to("any_user"));
}

#[test]
fn private_skill_visible_to_same_user() {
    let skill = test_skill(SkillScope::Private, "u1");
    assert!(skill.is_visible_to("u1"));
}

#[test]
fn private_skill_not_visible_to_different_user() {
    let skill = test_skill(SkillScope::Private, "u1");
    assert!(!skill.is_visible_to("u2"));
}

#[test]
fn compute_sha256_deterministic() {
    let skill = test_skill(SkillScope::Shared, "");
    let h1 = skill.compute_sha256();
    let h2 = skill.compute_sha256();
    assert_eq!(h1, h2);
}

#[test]
fn compute_sha256_changes_with_content() {
    let s1 = test_skill(SkillScope::Shared, "");
    let mut s2 = test_skill(SkillScope::Shared, "");
    s2.content = "different content".into();
    assert_ne!(s1.compute_sha256(), s2.compute_sha256());
}

#[test]
fn compute_sha256_includes_files() {
    let s1 = test_skill(SkillScope::Shared, "");
    let mut s2 = test_skill(SkillScope::Shared, "");
    s2.files = vec![SkillFile {
        path: "run.py".into(),
        body: "print('hi')".into(),
    }];
    assert_ne!(s1.compute_sha256(), s2.compute_sha256());
}

#[test]
fn compute_sha256_changes_with_version() {
    let s1 = test_skill(SkillScope::Shared, "");
    let mut s2 = test_skill(SkillScope::Shared, "");
    s2.version = "0.2.0".into();
    assert_ne!(s1.compute_sha256(), s2.compute_sha256());
}

#[test]
fn skill_serde_roundtrip() -> Result<()> {
    let skill = Skill {
        name: "test".into(),
        version: "1.0.0".into(),
        description: "desc".into(),
        scope: SkillScope::Private,
        source: SkillSource::Hub,
        user_id: "u1".to_string(),
        created_by: Some("u1".into()),
        last_used_by: None,
        timeout: 60,
        executable: true,
        parameters: vec![SkillParameter {
            name: "query".into(),
            description: "search query".into(),
            param_type: "string".into(),
            required: true,
            default: None,
        }],
        content: "code".into(),
        files: vec![SkillFile {
            path: "run.py".into(),
            body: "print()".into(),
        }],
        requires: Some(SkillRequirements {
            bins: vec!["python3".into()],
            env: vec!["API_KEY".into()],
        }),
        manifest: None,
    };
    let json = serde_json::to_string(&skill)?;
    let back: Skill = serde_json::from_str(&json)?;
    assert_eq!(back.name, "test");
    assert_eq!(back.scope, SkillScope::Private);
    assert_eq!(back.source, SkillSource::Hub);
    assert_eq!(back.parameters.len(), 1);
    assert_eq!(back.files.len(), 1);
    assert!(back.requires.is_some());
    Ok(())
}

#[test]
fn skill_requirements_default() {
    let r = SkillRequirements::default();
    assert!(r.bins.is_empty());
    assert!(r.env.is_empty());
}
