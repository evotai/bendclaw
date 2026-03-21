use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::skills::executor::SkillExecutor;
use bendclaw::kernel::skills::remote::writer;
use bendclaw::kernel::skills::runner::SkillRunner;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw::kernel::skills::store::SkillStore;
use tempfile::TempDir;

fn dummy_databases() -> Arc<bendclaw::storage::AgentDatabases> {
    let pool =
        bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool");
    Arc::new(bendclaw::storage::AgentDatabases::new(pool, "test_").unwrap())
}

fn dummy_pool() -> bendclaw::storage::Pool {
    bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool")
}

fn write_hub_skill(root: &std::path::Path, name: &str, version: &str, body: &str) -> Result<()> {
    let dir = root.join("skills").join(".hub").join(name);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {name}\nversion: {version}\ndescription: {name}\n---\n{body}"),
    )?;
    Ok(())
}

fn make_agent_skill(agent_id: &str, name: &str, description: &str, creator: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        scope: SkillScope::Agent,
        source: SkillSource::Agent,
        agent_id: Some(agent_id.to_string()),
        created_by: Some(creator.to_string()),
        timeout: 30,
        executable: true,
        parameters: vec![],
        content: format!("# {description}"),
        files: vec![SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho hi".to_string(),
        }],
        requires: None,
        manifest: None,
    }
}

fn make_agent_skill_with_files(
    agent_id: &str,
    name: &str,
    description: &str,
    creator: &str,
    files: Vec<SkillFile>,
) -> Skill {
    let mut skill = make_agent_skill(agent_id, name, description, creator);
    skill.files = files;
    skill
}

fn make_workspace(root: &std::path::Path, vars: &[(&str, &str)]) -> Arc<Workspace> {
    let dir = root.join("session");
    let _ = std::fs::create_dir_all(&dir);
    Arc::new(Workspace::new(
        dir.clone(),
        dir,
        vec!["PATH".into(), "HOME".into()],
        vars.iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
        Duration::from_secs(10),
        Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

#[test]
fn for_agent_deduplicates_agent_skill_over_hub_and_sorts_names() -> Result<()> {
    let workspace = TempDir::new()?;
    write_hub_skill(workspace.path(), "alpha", "1.0.0", "# alpha")?;
    write_hub_skill(workspace.path(), "dup", "1.0.0", "# hub dup")?;

    let agent_skill = make_agent_skill("agent-a", "dup", "agent dup", "user-1");
    writer::write_skill(workspace.path(), "agent-a", &agent_skill)
        .ok_or_else(|| anyhow::anyhow!("failed to write agent skill"))?;

    let beta_skill = make_agent_skill("agent-a", "beta", "beta", "user-1");
    writer::write_skill(workspace.path(), "agent-a", &beta_skill)
        .ok_or_else(|| anyhow::anyhow!("failed to write beta skill"))?;

    let store = SkillStore::new(dummy_databases(), workspace.path().to_path_buf(), None);
    let skills = store.for_agent("agent-a");

    assert_eq!(
        skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["alpha", "beta", "dup"]
    );
    assert_eq!(
        skills
            .iter()
            .find(|s| s.name == "dup")
            .map(|s| s.description.as_str()),
        Some("agent dup")
    );
    Ok(())
}

#[test]
fn get_hub_reads_latest_version_from_versioned_layout() -> Result<()> {
    let workspace = TempDir::new()?;
    let skill_root = workspace.path().join("skills").join(".hub").join("tool");
    std::fs::create_dir_all(skill_root.join("1.0.0"))?;
    std::fs::create_dir_all(skill_root.join("2.0.0"))?;
    std::fs::write(
        skill_root.join("1.0.0").join("SKILL.md"),
        "---\nname: tool\nversion: 1.0.0\ndescription: tool\n---\nold",
    )?;
    std::fs::write(
        skill_root.join("2.0.0").join("SKILL.md"),
        "---\nname: tool\nversion: 2.0.0\ndescription: tool\n---\nnew",
    )?;

    let store = SkillStore::new(dummy_databases(), workspace.path().to_path_buf(), None);
    let skill = store
        .get_hub("tool")
        .ok_or_else(|| anyhow::anyhow!("hub skill not found"))?;

    assert_eq!(skill.version, "2.0.0");
    assert_eq!(store.read_skill("agent-a", "tool"), Some("new".to_string()));
    Ok(())
}

#[test]
fn disk_checksum_changes_when_creator_changes() -> Result<()> {
    let workspace = TempDir::new()?;
    let skill_v1 = make_agent_skill("agent-a", "creator-skill", "same body", "user-1");
    writer::write_skill(workspace.path(), "agent-a", &skill_v1)
        .ok_or_else(|| anyhow::anyhow!("failed to write first version"))?;
    let checksum_v1 = writer::read_disk_checksum(workspace.path(), "agent-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("missing checksum v1"))?;

    let skill_v2 = make_agent_skill("agent-a", "creator-skill", "same body", "user-2");
    writer::write_skill(workspace.path(), "agent-a", &skill_v2)
        .ok_or_else(|| anyhow::anyhow!("failed to write second version"))?;
    let checksum_v2 = writer::read_disk_checksum(workspace.path(), "agent-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("missing checksum v2"))?;

    let store = SkillStore::new(dummy_databases(), workspace.path().to_path_buf(), None);
    let loaded = store
        .get("agent-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;

    assert_ne!(checksum_v1, checksum_v2);
    assert_eq!(loaded.created_by.as_deref(), Some("user-2"));
    Ok(())
}

#[test]
fn same_agent_same_name_overwrite_replaces_files_and_creator() -> Result<()> {
    let workspace = TempDir::new()?;
    let store = SkillStore::new(dummy_databases(), workspace.path().to_path_buf(), None);

    let skill_v1 = make_agent_skill_with_files("agent-a", "dup-skill", "first", "user-1", vec![
        SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho first".to_string(),
        },
        SkillFile {
            path: "references/old.md".to_string(),
            body: "# old".to_string(),
        },
    ]);
    store.insert(&skill_v1, "agent-a");

    let skill_v2 = make_agent_skill_with_files("agent-a", "dup-skill", "second", "user-2", vec![
        SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho second".to_string(),
        },
        SkillFile {
            path: "references/new.md".to_string(),
            body: "# new".to_string(),
        },
    ]);
    store.insert(&skill_v2, "agent-a");

    let loaded = store
        .get("agent-a", "dup-skill")
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    assert_eq!(loaded.description, "second");
    assert_eq!(loaded.created_by.as_deref(), Some("user-2"));
    assert_eq!(
        store.read_skill("agent-a", "dup-skill/references/new.md"),
        Some("# new".to_string())
    );
    assert_eq!(
        store.read_skill("agent-a", "dup-skill/references/old.md"),
        None
    );
    Ok(())
}

#[test]
fn same_name_can_exist_under_different_agents() -> Result<()> {
    let workspace = TempDir::new()?;
    let store = SkillStore::new(dummy_databases(), workspace.path().to_path_buf(), None);

    store.insert(
        &make_agent_skill("agent-a", "shared-name", "agent a", "user-a"),
        "agent-a",
    );
    store.insert(
        &make_agent_skill("agent-b", "shared-name", "agent b", "user-b"),
        "agent-b",
    );

    let skill_a = store
        .get("agent-a", "shared-name")
        .ok_or_else(|| anyhow::anyhow!("agent-a skill missing"))?;
    let skill_b = store
        .get("agent-b", "shared-name")
        .ok_or_else(|| anyhow::anyhow!("agent-b skill missing"))?;

    assert_eq!(skill_a.description, "agent a");
    assert_eq!(skill_a.created_by.as_deref(), Some("user-a"));
    assert_eq!(skill_b.description, "agent b");
    assert_eq!(skill_b.created_by.as_deref(), Some("user-b"));
    Ok(())
}

#[tokio::test]
async fn hub_and_remote_updates_are_visible_and_remote_skill_reads_variables() -> Result<()> {
    let workspace = TempDir::new()?;
    let hub_dir = workspace.path().join("skills").join(".hub").join("docs");
    std::fs::create_dir_all(hub_dir.join("references"))?;
    std::fs::write(
        hub_dir.join("SKILL.md"),
        "---\nname: docs\nversion: 1.0.0\ndescription: docs\n---\n# Hub V1",
    )?;
    std::fs::write(hub_dir.join("references/guide.md"), "# Guide V1")?;

    let store = Arc::new(SkillStore::new(
        dummy_databases(),
        workspace.path().to_path_buf(),
        None,
    ));
    assert_eq!(
        store.read_skill("agent-a", "docs"),
        Some("# Hub V1".to_string())
    );
    assert_eq!(
        store.read_skill("agent-a", "docs/references/guide.md"),
        Some("# Guide V1".to_string())
    );

    let mut remote_skill = make_agent_skill("agent-a", "remote-tool", "remote tool", "user-1");
    remote_skill.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    remote_skill.files = vec![SkillFile {
        path: "scripts/run.sh".to_string(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".to_string(),
    }];
    store.insert(&remote_skill, "agent-a");

    let runner = SkillRunner::new(
        "agent-a",
        "user-1",
        store.clone(),
        make_workspace(workspace.path(), &[("API_TOKEN", "token-v1")]),
        dummy_pool(),
    );
    let output = runner.execute("remote-tool", &[]).await?;
    assert_eq!(output.data, Some(serde_json::json!("token-v1")));

    let mut remote_skill_v2 =
        make_agent_skill("agent-a", "remote-tool", "remote tool updated", "user-2");
    remote_skill_v2.requires = Some(bendclaw::kernel::skills::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    remote_skill_v2.files = vec![SkillFile {
        path: "scripts/run.sh".to_string(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf 'updated:%s' \"$API_TOKEN\"".to_string(),
    }];
    store.insert(&remote_skill_v2, "agent-a");
    let output = runner.execute("remote-tool", &[]).await?;
    assert_eq!(output.data, Some(serde_json::json!("updated:token-v1")));

    std::fs::write(
        hub_dir.join("SKILL.md"),
        "---\nname: docs\nversion: 1.0.1\ndescription: docs\n---\n# Hub V2",
    )?;
    std::fs::write(hub_dir.join("references/guide.md"), "# Guide V2")?;

    assert_eq!(
        store.read_skill("agent-a", "docs"),
        Some("# Hub V2".to_string())
    );
    assert_eq!(
        store.read_skill("agent-a", "docs/references/guide.md"),
        Some("# Guide V2".to_string())
    );
    Ok(())
}
