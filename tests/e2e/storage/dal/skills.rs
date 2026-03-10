//! Integration tests for [`DatabendSkillRepository`]: CRUD and scope visibility.

use anyhow::Result;
use bendclaw::kernel::skills::repository::DatabendSkillRepository;
use bendclaw::kernel::skills::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillFile;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw_test_harness::setup::pool;
use bendclaw_test_harness::setup::uid;

fn make_skill(name: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "a test skill".to_string(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "# docs".to_string(),
        files: vec![],
        requires: None,
    }
}

fn make_agent_skill(name: &str, agent_id: &str, user_id: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "agent skill".to_string(),
        scope: SkillScope::Agent,
        source: SkillSource::Agent,
        agent_id: Some(agent_id.to_string()),
        user_id: Some(user_id.to_string()),
        timeout: 30,
        executable: true,
        parameters: vec![],
        content: "# agent docs".to_string(),
        files: vec![],
        requires: None,
    }
}

// ── CRUD ──────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn save_and_get_roundtrip() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let s = make_skill(&uid("sk"));

    store.save(&s).await?;
    let got = store
        .get(&s.name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill must exist after save"))?;

    assert_eq!(got.name, s.name);
    assert_eq!(got.version, s.version);
    assert_eq!(got.description, s.description);
    assert_eq!(got.scope, SkillScope::Global);
    assert_eq!(got.source, SkillSource::Local);
    assert!(got.agent_id.is_none());
    assert!(got.user_id.is_none());
    Ok(())
}

#[tokio::test]
async fn get_unknown_returns_none() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let got = store.get(&uid("sk")).await?;
    assert!(got.is_none());
    Ok(())
}

#[tokio::test]
async fn save_overwrites_existing() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let name = uid("sk");

    let mut v1 = make_skill(&name);
    v1.version = "1.0.0".to_string();
    store.save(&v1).await?;

    let mut v2 = make_skill(&name);
    v2.version = "2.0.0".to_string();
    store.save(&v2).await?;

    let got = store
        .get(&name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;
    assert_eq!(got.version, "2.0.0", "second save must overwrite first");
    Ok(())
}

#[tokio::test]
async fn list_returns_all_saved_skills() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let prefix = uid("sk");
    let names: Vec<String> = (0..3).map(|i| format!("{prefix}-{i}")).collect();

    for name in &names {
        store.save(&make_skill(name)).await?;
    }
    let listed = store.list().await?;

    assert!(listed.len() >= 3);
    for name in &names {
        assert!(
            listed.iter().any(|s| s.name == *name),
            "expected skill '{name}' in list"
        );
    }
    Ok(())
}

#[tokio::test]
async fn save_with_files_roundtrip() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let mut s = make_skill(&uid("sk"));
    s.files = vec![SkillFile {
        path: "run.sh".to_string(),
        body: "#!/bin/bash\necho hello".to_string(),
    }];

    store.save(&s).await?;
    let got = store
        .get(&s.name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill not found"))?;

    assert_eq!(got.files.len(), 1);
    assert_eq!(got.files[0].path, "run.sh");
    assert_eq!(got.files[0].body, "#!/bin/bash\necho hello");
    Ok(())
}

#[tokio::test]
async fn remove_deletes_skill_and_files() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let mut s = make_skill(&uid("sk"));
    s.files = vec![SkillFile {
        path: "f.txt".to_string(),
        body: "body".to_string(),
    }];

    store.save(&s).await?;
    store.remove(&s.name, None, None).await?;
    let got = store.get(&s.name).await?;

    assert!(got.is_none(), "skill must not exist after remove");
    Ok(())
}

#[tokio::test]
async fn checksums_contains_sha256_of_saved_skill() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let s = make_skill(&uid("sk"));
    let expected_sha = s.compute_sha256();

    store.save(&s).await?;
    let checksums = store.checksums().await?;

    assert_eq!(
        checksums.get(&s.name).map(String::as_str),
        Some(expected_sha.as_str())
    );
    Ok(())
}

#[tokio::test]
async fn remove_is_idempotent() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let s = make_skill(&uid("sk"));

    store.save(&s).await?;
    store.remove(&s.name, None, None).await?;
    store.remove(&s.name, None, None).await?;
    let got = store.get(&s.name).await?;

    assert!(got.is_none());
    Ok(())
}

// ── Scope persistence ─────────────────────────────────────────────────────────

#[tokio::test]
async fn agent_skill_persists_scope_and_ownership() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let s = make_agent_skill(&uid("sk"), "agent-1", "user-1");

    store.save(&s).await?;
    let got = store
        .get(&s.name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill must exist"))?;

    assert_eq!(got.scope, SkillScope::Agent);
    assert_eq!(got.source, SkillSource::Agent);
    assert_eq!(got.agent_id.as_deref(), Some("agent-1"));
    assert_eq!(got.user_id.as_deref(), Some("user-1"));
    Ok(())
}

#[tokio::test]
async fn list_returns_skills_of_all_scopes() -> Result<()> {
    let store = DatabendSkillRepository::new(pool().await?);
    let global_name = uid("sk-global");
    let agent_name = uid("sk-agent");

    store.save(&make_skill(&global_name)).await?;
    store
        .save(&make_agent_skill(&agent_name, "agent-1", "user-1"))
        .await?;

    let listed = store.list().await?;
    assert!(listed.iter().any(|s| s.name == global_name));
    assert!(listed.iter().any(|s| s.name == agent_name));
    Ok(())
}
