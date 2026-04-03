use anyhow::Result;
use bendclaw::skills::definition::skill::Skill;
use bendclaw::skills::definition::skill::SkillFile;
use bendclaw::skills::definition::skill::SkillRequirements;
use bendclaw::skills::definition::skill::SkillScope;
use bendclaw::skills::definition::skill::SkillSource;
use bendclaw::skills::store::DatabendSharedSkillStore;
use bendclaw::skills::store::SharedSkillStore;
use bendclaw::storage::Pool;

use crate::common::setup::require_api_config;
use crate::common::setup::uid;
use crate::common::setup::TestContext;

fn make_skill(user_id: &str, name: &str, creator: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "remote skill".to_string(),
        scope: SkillScope::Private,
        source: SkillSource::Agent,
        user_id: user_id.to_string(),
        created_by: Some(creator.to_string()),
        last_used_by: None,
        timeout: 45,
        executable: true,
        parameters: vec![],
        content: "# Remote Skill".to_string(),
        files: vec![
            SkillFile {
                path: "references/usage.md".to_string(),
                body: "# Usage".to_string(),
            },
            SkillFile {
                path: "scripts/run.sh".to_string(),
                body: "#!/usr/bin/env bash\necho sync".to_string(),
            },
        ],
        requires: Some(SkillRequirements {
            bins: vec!["bash".to_string()],
            env: vec!["API_TOKEN".to_string()],
        }),
        manifest: None,
    }
}

async fn setup_meta_pool() -> Result<Option<Pool>> {
    let (base_url, token, warehouse) = require_api_config()?;
    if token.is_empty() {
        return Ok(None);
    }
    let root = Pool::new(&base_url, &token, &warehouse)?;
    root.exec("CREATE DATABASE IF NOT EXISTS evotai_meta")
        .await?;
    let meta_pool = root.with_database("evotai_meta")?;
    Ok(Some(meta_pool))
}

#[tokio::test]
async fn shared_skill_store_save_and_get_roundtrip() -> Result<()> {
    let _ctx = TestContext::setup().await?;
    let Some(meta_pool) = setup_meta_pool().await? else {
        eprintln!("skipping shared skill store test: missing config");
        return Ok(());
    };
    let store = DatabendSharedSkillStore::new(meta_pool);

    let user_id = uid("user");
    let skill_name = uid("shared-skill");
    let skill = make_skill(&user_id, &skill_name, &user_id);
    store.save(&user_id, &skill).await?;

    let loaded = store
        .get(&user_id, &skill_name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("skill not found after save"))?;
    assert_eq!(loaded.name, skill_name);
    assert_eq!(loaded.description, "remote skill");

    store.remove(&user_id, &skill_name).await?;
    assert!(store.get(&user_id, &skill_name).await?.is_none());
    Ok(())
}
