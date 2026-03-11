use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::catalog::SkillCatalogImpl;
use bendclaw::kernel::skills::fs::SkillMirror;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;

fn make_skill(name: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "desc".to_string(),
        scope: SkillScope::Global,
        source: SkillSource::Agent,
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "# skill".to_string(),
        files: vec![],
        requires: None,
    }
}

fn make_catalog(dir: &std::path::Path) -> Result<SkillCatalogImpl, Box<dyn std::error::Error>> {
    let pool = bendclaw::storage::pool::Pool::new(
        "https://app.databend.com/v1.1",
        "dummy-token",
        "default",
    )?;
    let databases = std::sync::Arc::new(bendclaw::storage::AgentDatabases::new(
        pool,
        "test_mirror_",
    )?);
    Ok(SkillCatalogImpl::new(databases, dir.to_path_buf()))
}

#[test]
fn insert_ignores_unsafe_file_paths_but_keeps_skill() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    let mut skill = make_skill("safe-skill");
    skill.files = vec![
        SkillFile {
            path: "scripts/run.py".to_string(),
            body: "print('ok')".to_string(),
        },
        SkillFile {
            path: "../evil.txt".to_string(),
            body: "bad".to_string(),
        },
        SkillFile {
            path: "/tmp/evil.txt".to_string(),
            body: "bad".to_string(),
        },
    ];

    catalog.insert(&skill);

    let loaded = catalog.get("safe-skill").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "safe-skill missing after insert",
        )
    })?;
    assert_eq!(loaded.files.len(), 3);
    let skill_dir = tmp.path().join(".remote").join("safe-skill");

    assert!(skill_dir.join("scripts/run.py").exists());
    assert!(!tmp.path().join(".remote").join("evil.txt").exists());
    assert!(!tmp.path().join("evil.txt").exists());
    Ok(())
}

#[test]
fn evict_rejects_traversal_name() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let catalog = make_catalog(tmp.path())?;

    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside)?;

    // Traversal-like name should be ignored and not touch sibling directory.
    catalog.evict("../outside");
    assert!(outside.exists());
    Ok(())
}

// ── SkillMirror ──

fn make_remote_skill(name: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".into(),
        description: "desc".into(),
        scope: SkillScope::Global,
        source: SkillSource::Hub,
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: true,
        parameters: vec![],
        content: "# remote skill".into(),
        files: vec![SkillFile {
            path: "scripts/run.py".into(),
            body: "print('hello')".into(),
        }],
        requires: None,
    }
}

#[test]
fn skill_mirror_skill_dir_local_uses_builtins() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    let mut skill = make_remote_skill("my-skill");
    skill.source = SkillSource::Local;
    let dir = mirror.skill_dir(&skill);
    assert_eq!(dir, mirror.builtins_dir.join("my-skill"));
    Ok(())
}

#[test]
fn skill_mirror_skill_dir_remote_uses_remote() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    let skill = make_remote_skill("hub-skill");
    let dir = mirror.skill_dir(&skill);
    assert_eq!(dir, mirror.remote_dir.join("hub-skill"));
    Ok(())
}

#[test]
fn skill_mirror_write_remote_creates_files() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    let skill = make_remote_skill("remote-skill");
    let loaded = mirror.write_remote_skill("remote-skill", &skill);
    assert!(loaded.is_some());
    assert!(mirror.remote_dir.join("remote-skill/SKILL.md").exists());
    assert!(mirror
        .remote_dir
        .join("remote-skill/scripts/run.py")
        .exists());
    Ok(())
}

#[test]
fn skill_mirror_write_remote_overwrites_existing() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    mirror.write_remote_skill("ow-skill", &make_remote_skill("ow-skill"));
    let mut updated = make_remote_skill("ow-skill");
    updated.version = "2.0.0".into();
    mirror.write_remote_skill("ow-skill", &updated);
    let skill_md = std::fs::read_to_string(mirror.remote_dir.join("ow-skill/SKILL.md"))?;
    assert!(skill_md.contains("2.0.0"));
    Ok(())
}

#[test]
fn skill_mirror_remove_remote_deletes_dir() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    mirror.write_remote_skill("to-remove", &make_remote_skill("to-remove"));
    assert!(mirror.remote_dir.join("to-remove").exists());
    mirror.remove_remote_skill("to-remove");
    assert!(!mirror.remote_dir.join("to-remove").exists());
    Ok(())
}

#[test]
fn skill_mirror_remove_remote_nonexistent_is_noop() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    mirror.remove_remote_skill("does-not-exist");
    Ok(())
}

#[test]
fn skill_mirror_remove_remote_rejects_traversal() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempfile::TempDir::new()?;
    let mirror = SkillMirror::new(tmp.path().to_path_buf());
    let sibling = tmp.path().join("sibling");
    std::fs::create_dir_all(&sibling)?;
    mirror.remove_remote_skill("../sibling");
    assert!(sibling.exists(), "traversal removal must be rejected");
    Ok(())
}
