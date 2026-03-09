use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::catalog::SkillCatalogImpl;
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
    let pool =
        bendclaw::storage::pool::Pool::new("https://api.databend.com/v1", "dummy-token", "default")?;
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
