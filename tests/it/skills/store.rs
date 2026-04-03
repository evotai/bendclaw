use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use bendclaw::execution::skills::SkillExecutor;
use bendclaw::execution::skills::SkillRunner;
use bendclaw::sessions::workspace::SandboxResolver;
use bendclaw::sessions::workspace::Workspace;
use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::definition::skill::SkillFile;
use bendclaw::skills::definition::skill::SkillScope;
use bendclaw::skills::definition::skill::SkillSource;
use bendclaw::skills::sources::remote::writer;
use bendclaw::skills::sync::SkillIndex;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use bendclaw_test_harness::mocks::skill::NoopUsageSink;
use tempfile::TempDir;

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

fn make_user_skill(user_id: &str, name: &str, description: &str, creator: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: description.to_string(),
        scope: SkillScope::Private,
        source: SkillSource::Agent,
        user_id: user_id.to_string(),
        created_by: Some(creator.to_string()),
        last_used_by: None,
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

fn make_user_skill_with_files(
    user_id: &str,
    name: &str,
    description: &str,
    creator: &str,
    files: Vec<SkillFile>,
) -> Skill {
    let mut skill = make_user_skill(user_id, name, description, creator);
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
fn for_user_deduplicates_user_skill_over_hub_and_sorts_names() -> Result<()> {
    let workspace = TempDir::new()?;
    write_hub_skill(workspace.path(), "alpha", "1.0.0", "# alpha")?;
    write_hub_skill(workspace.path(), "dup", "1.0.0", "# hub dup")?;

    let user_skill = make_user_skill("user-a", "dup", "agent dup", "user-1");
    writer::write_skill(workspace.path(), "user-a", &user_skill)
        .ok_or_else(|| anyhow::anyhow!("failed to write user skill"))?;

    let beta_skill = make_user_skill("user-a", "beta", "beta", "user-1");
    writer::write_skill(workspace.path(), "user-a", &beta_skill)
        .ok_or_else(|| anyhow::anyhow!("failed to write beta skill"))?;

    let catalog = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );
    let skills = catalog.visible_skills("user-a");

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

    let catalog = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );
    let skill = catalog
        .get_hub("tool")
        .ok_or_else(|| anyhow::anyhow!("hub skill not found"))?;

    assert_eq!(skill.version, "2.0.0");
    assert_eq!(
        catalog.read_skill("user-a", "tool"),
        Some("new".to_string())
    );
    Ok(())
}

#[test]
fn disk_checksum_changes_when_creator_changes() -> Result<()> {
    let workspace = TempDir::new()?;
    let skill_v1 = make_user_skill("user-a", "creator-skill", "same body", "user-1");
    writer::write_skill(workspace.path(), "user-a", &skill_v1)
        .ok_or_else(|| anyhow::anyhow!("failed to write first version"))?;
    let checksum_v1 = writer::read_disk_checksum(workspace.path(), "user-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("missing checksum v1"))?;

    let skill_v2 = make_user_skill("user-a", "creator-skill", "same body", "user-2");
    writer::write_skill(workspace.path(), "user-a", &skill_v2)
        .ok_or_else(|| anyhow::anyhow!("failed to write second version"))?;
    let checksum_v2 = writer::read_disk_checksum(workspace.path(), "user-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("missing checksum v2"))?;

    let catalog = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );
    let loaded = catalog
        .resolve("user-a", "creator-skill")
        .ok_or_else(|| anyhow::anyhow!("skill not loaded"))?;

    assert_ne!(checksum_v1, checksum_v2);
    assert_eq!(loaded.created_by.as_deref(), Some("user-2"));
    Ok(())
}

#[test]
fn same_user_same_name_overwrite_replaces_files_and_creator() -> Result<()> {
    let workspace = TempDir::new()?;

    let skill_v1 = make_user_skill_with_files("user-a", "dup-skill", "first", "user-1", vec![
        SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho first".to_string(),
        },
        SkillFile {
            path: "references/old.md".to_string(),
            body: "# old".to_string(),
        },
    ]);
    writer::write_skill(workspace.path(), "user-a", &skill_v1);

    let skill_v2 = make_user_skill_with_files("user-a", "dup-skill", "second", "user-2", vec![
        SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho second".to_string(),
        },
        SkillFile {
            path: "references/new.md".to_string(),
            body: "# new".to_string(),
        },
    ]);
    writer::write_skill(workspace.path(), "user-a", &skill_v2);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );
    let loaded = projector
        .resolve("user-a", "dup-skill")
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    assert_eq!(loaded.description, "second");
    assert_eq!(loaded.created_by.as_deref(), Some("user-2"));
    assert_eq!(
        projector.read_skill("user-a", "dup-skill/references/new.md"),
        Some("# new".to_string())
    );
    assert_eq!(
        projector.read_skill("user-a", "dup-skill/references/old.md"),
        None
    );
    Ok(())
}

#[test]
fn same_name_can_exist_under_different_users() -> Result<()> {
    let workspace = TempDir::new()?;

    writer::write_skill(
        workspace.path(),
        "user-a",
        &make_user_skill("user-a", "shared-name", "user a", "user-a"),
    );
    writer::write_skill(
        workspace.path(),
        "user-b",
        &make_user_skill("user-b", "shared-name", "user b", "user-b"),
    );

    let projector_a = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );
    let skill_a = projector_a
        .resolve("user-a", "shared-name")
        .ok_or_else(|| anyhow::anyhow!("user-a skill missing"))?;
    let skill_b = projector_a
        .resolve("user-b", "shared-name")
        .ok_or_else(|| anyhow::anyhow!("user-b skill missing"))?;

    assert_eq!(skill_a.description, "user a");
    assert_eq!(skill_a.created_by.as_deref(), Some("user-a"));
    assert_eq!(skill_b.description, "user b");
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

    let projector = Arc::new(SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    assert_eq!(
        projector.read_skill("user-a", "docs"),
        Some("# Hub V1".to_string())
    );
    assert_eq!(
        projector.read_skill("user-a", "docs/references/guide.md"),
        Some("# Guide V1".to_string())
    );

    let mut remote_skill = make_user_skill("user-a", "remote-tool", "remote tool", "user-1");
    remote_skill.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    remote_skill.files = vec![SkillFile {
        path: "scripts/run.sh".to_string(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf '%s' \"$API_TOKEN\"".to_string(),
    }];
    writer::write_skill(workspace.path(), "user-a", &remote_skill);

    let runner = SkillRunner::new(
        "agent-a",
        "user-a",
        projector.clone(),
        Arc::new(NoopUsageSink),
        make_workspace(workspace.path(), &[("API_TOKEN", "token-v1")]),
        dummy_pool(),
    );
    let output = runner.execute("remote-tool", &[]).await?;
    assert_eq!(output.data, Some(serde_json::json!("token-v1")));

    let mut remote_skill_v2 =
        make_user_skill("user-a", "remote-tool", "remote tool updated", "user-2");
    remote_skill_v2.requires = Some(bendclaw::skills::definition::skill::SkillRequirements {
        bins: vec!["bash".into()],
        env: vec!["API_TOKEN".into()],
    });
    remote_skill_v2.files = vec![SkillFile {
        path: "scripts/run.sh".to_string(),
        body: "#!/usr/bin/env bash\ncat >/dev/null\nprintf 'updated:%s' \"$API_TOKEN\"".to_string(),
    }];
    writer::write_skill(workspace.path(), "user-a", &remote_skill_v2);
    let output = runner.execute("remote-tool", &[]).await?;
    assert_eq!(output.data, Some(serde_json::json!("updated:token-v1")));

    std::fs::write(
        hub_dir.join("SKILL.md"),
        "---\nname: docs\nversion: 1.0.1\ndescription: docs\n---\n# Hub V2",
    )?;
    std::fs::write(hub_dir.join("references/guide.md"), "# Guide V2")?;

    assert_eq!(
        projector.read_skill("user-a", "docs"),
        Some("# Hub V2".to_string())
    );
    assert_eq!(
        projector.read_skill("user-a", "docs/references/guide.md"),
        Some("# Guide V2".to_string())
    );
    Ok(())
}

// ── Subscribed skill contract tests ─────────────────────────────────────────

#[test]
fn subscribed_skill_visible_under_namespaced_key() -> Result<()> {
    let workspace = TempDir::new()?;

    let skill = make_user_skill("alice", "report", "alice report", "alice");
    writer::write_subscribed_skill(workspace.path(), "bob", "alice", &skill);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );

    let resolved = projector
        .resolve("bob", "alice/report")
        .ok_or_else(|| anyhow::anyhow!("subscribed skill not found via alice/report"))?;
    assert_eq!(resolved.name, "report");
    assert_eq!(resolved.user_id, "alice");
    assert_eq!(resolved.description, "alice report");

    assert!(projector.resolve("bob", "report").is_none());

    Ok(())
}

#[test]
fn subscribed_skill_in_visible_list_with_tool_key() -> Result<()> {
    let workspace = TempDir::new()?;

    let owned = make_user_skill("bob", "my-tool", "bob tool", "bob");
    writer::write_skill(workspace.path(), "bob", &owned);

    let subscribed = make_user_skill("alice", "shared-tool", "alice tool", "alice");
    writer::write_subscribed_skill(workspace.path(), "bob", "alice", &subscribed);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );

    let skills = projector.visible_skills("bob");
    let names: Vec<String> = skills
        .iter()
        .map(|s| bendclaw::skills::definition::tool_key::format(s, "bob"))
        .collect();

    assert!(names.contains(&"my-tool".to_string()));
    assert!(names.contains(&"alice/shared-tool".to_string()));

    for name in &names {
        assert!(
            projector.resolve("bob", name).is_some(),
            "resolve failed for tool key: {name}"
        );
    }

    Ok(())
}

#[test]
fn same_name_subscribed_from_different_owners_both_visible_and_stable() -> Result<()> {
    let workspace = TempDir::new()?;

    let alice_skill = make_user_skill("alice", "report", "alice report", "alice");
    writer::write_subscribed_skill(workspace.path(), "viewer", "alice", &alice_skill);

    let charlie_skill = make_user_skill("charlie", "report", "charlie report", "charlie");
    writer::write_subscribed_skill(workspace.path(), "viewer", "charlie", &charlie_skill);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );

    let skills = projector.visible_skills("viewer");
    let keys: Vec<String> = skills
        .iter()
        .map(|s| bendclaw::skills::definition::tool_key::format(s, "viewer"))
        .collect();

    assert!(keys.contains(&"alice/report".to_string()));
    assert!(keys.contains(&"charlie/report".to_string()));

    let alice_pos = keys.iter().position(|k| k == "alice/report").unwrap();
    let charlie_pos = keys.iter().position(|k| k == "charlie/report").unwrap();
    assert!(alice_pos < charlie_pos);

    let a = projector.resolve("viewer", "alice/report").unwrap();
    let c = projector.resolve("viewer", "charlie/report").unwrap();
    assert_eq!(a.description, "alice report");
    assert_eq!(c.description, "charlie report");

    Ok(())
}

#[test]
fn subscribed_skill_read_skill_with_doc_path() -> Result<()> {
    let workspace = TempDir::new()?;

    let mut skill = make_user_skill("alice", "docs", "alice docs", "alice");
    skill.files = vec![
        SkillFile {
            path: "scripts/run.sh".to_string(),
            body: "#!/usr/bin/env bash\necho hi".to_string(),
        },
        SkillFile {
            path: "references/guide.md".to_string(),
            body: "# Alice Guide".to_string(),
        },
    ];
    writer::write_subscribed_skill(workspace.path(), "bob", "alice", &skill);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );

    assert_eq!(
        projector.read_skill("bob", "alice/docs"),
        Some("# alice docs".to_string())
    );
    assert_eq!(
        projector.read_skill("bob", "alice/docs/references/guide.md"),
        Some("# Alice Guide".to_string())
    );

    Ok(())
}

#[test]
fn owned_skill_overrides_hub_but_subscribed_coexists() -> Result<()> {
    let workspace = TempDir::new()?;

    write_hub_skill(workspace.path(), "report", "1.0.0", "# hub report")?;

    let owned = make_user_skill("bob", "report", "bob report", "bob");
    writer::write_skill(workspace.path(), "bob", &owned);

    let subscribed = make_user_skill("alice", "report", "alice report", "alice");
    writer::write_subscribed_skill(workspace.path(), "bob", "alice", &subscribed);

    let projector = SkillIndex::new(
        workspace.path().to_path_buf(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    );

    let skills = projector.visible_skills("bob");
    let keys: Vec<String> = skills
        .iter()
        .map(|s| bendclaw::skills::definition::tool_key::format(s, "bob"))
        .collect();

    let report = projector.resolve("bob", "report").unwrap();
    assert_eq!(report.description, "bob report");

    let alice_report = projector.resolve("bob", "alice/report").unwrap();
    assert_eq!(alice_report.description, "alice report");

    assert!(keys.contains(&"report".to_string()));
    assert!(keys.contains(&"alice/report".to_string()));

    Ok(())
}
