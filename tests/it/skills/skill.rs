use anyhow::Result;
use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::definition::skill::SkillFile;
use bendclaw::skills::definition::skill::SkillParameter;
use bendclaw::skills::definition::skill::SkillRequirements;
use bendclaw::skills::definition::skill::SkillScope;
use bendclaw::skills::definition::skill::SkillSource;

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

// ── Skill::validate_name ──

#[test]
fn validate_name_valid() {
    assert!(Skill::validate_name("my-skill").is_ok());
    assert!(Skill::validate_name("ab").is_ok());
    assert!(Skill::validate_name("skill123").is_ok());
    assert!(Skill::validate_name("a-b-c").is_ok());
}

#[test]
fn validate_name_too_short() {
    assert!(Skill::validate_name("a").is_err());
}

#[test]
fn validate_name_too_long() {
    let long = "a".repeat(65);
    assert!(Skill::validate_name(&long).is_err());
}

#[test]
fn validate_name_starts_with_dash() {
    assert!(Skill::validate_name("-bad").is_err());
}

#[test]
fn validate_name_ends_with_dash() {
    assert!(Skill::validate_name("bad-").is_err());
}

#[test]
fn validate_name_uppercase_rejected() {
    assert!(Skill::validate_name("MySkill").is_err());
}

#[test]
fn validate_name_path_traversal_rejected() {
    assert!(Skill::validate_name("a..b").is_err());
    assert!(Skill::validate_name("a/b").is_err());
}

#[test]
fn validate_name_reserved_tool_name_rejected() {
    assert!(Skill::validate_name("bash").is_err());
}

// ── Skill::validate_file_path ──

#[test]
fn validate_file_path_valid_script() {
    assert!(Skill::validate_file_path("scripts/run.py").is_ok());
    assert!(Skill::validate_file_path("scripts/run.sh").is_ok());
}

#[test]
fn validate_file_path_valid_reference() {
    assert!(Skill::validate_file_path("references/guide.md").is_ok());
}

#[test]
fn validate_file_path_empty_rejected() {
    assert!(Skill::validate_file_path("").is_err());
}

#[test]
fn validate_file_path_absolute_rejected() {
    assert!(Skill::validate_file_path("/etc/passwd").is_err());
    assert!(Skill::validate_file_path("\\windows\\path").is_err());
}

#[test]
fn validate_file_path_traversal_rejected() {
    assert!(Skill::validate_file_path("scripts/../etc/passwd").is_err());
}

#[test]
fn validate_file_path_bad_prefix_rejected() {
    assert!(Skill::validate_file_path("data/file.md").is_err());
}

#[test]
fn validate_file_path_bad_extension_rejected() {
    assert!(Skill::validate_file_path("scripts/run.js").is_err());
    assert!(Skill::validate_file_path("references/guide.txt").is_err());
}

// ── Skill::validate_size ──

#[test]
fn validate_size_within_limits() {
    assert!(Skill::validate_size("small content", &[]).is_ok());
}

#[test]
fn validate_size_content_too_large() {
    let big = "x".repeat(10 * 1024 + 1);
    assert!(Skill::validate_size(&big, &[]).is_err());
}

#[test]
fn validate_size_single_file_too_large() {
    let big_body = "x".repeat(50 * 1024 + 1);
    let files = vec![SkillFile {
        path: "scripts/run.py".into(),
        body: big_body,
    }];
    assert!(Skill::validate_size("ok", &files).is_err());
}

#[test]
fn validate_size_total_files_too_large() {
    let body = "x".repeat(41 * 1024);
    let files: Vec<SkillFile> = (0..5)
        .map(|i| SkillFile {
            path: format!("scripts/run{i}.py"),
            body: body.clone(),
        })
        .collect();
    assert!(Skill::validate_size("ok", &files).is_err());
}

// ── Skill::validate (full) ──

#[test]
fn validate_valid_skill() {
    let skill = test_skill(SkillScope::Shared, "");
    assert!(skill.validate().is_ok());
}

#[test]
fn validate_invalid_name_fails() {
    let mut skill = test_skill(SkillScope::Shared, "");
    skill.name = "INVALID".into();
    assert!(skill.validate().is_err());
}

#[test]
fn validate_invalid_file_path_fails() {
    let mut skill = test_skill(SkillScope::Shared, "");
    skill.files = vec![SkillFile {
        path: "/absolute/path.py".into(),
        body: "code".into(),
    }];
    assert!(skill.validate().is_err());
}

#[test]
fn validate_content_too_large_fails() {
    let mut skill = test_skill(SkillScope::Shared, "");
    skill.content = "x".repeat(10 * 1024 + 1);
    assert!(skill.validate().is_err());
}
