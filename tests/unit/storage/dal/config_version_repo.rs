use anyhow::Result;
use bendclaw::storage::config_version::ConfigVersionRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn version_row(id: &str, version: &str) -> Vec<serde_json::Value> {
    vec![
        id,
        "agent-1",
        version,
        "v1",
        "production",
        "You are helpful.",
        "Bot",
        "A bot",
        "assistant",
        "friendly",
        "",
        "",
        "",
        "initial",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn version_response(id: &str, version: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".into(),
        error: None,
        data: vec![version_row(id, version)],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn config_version_insert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO agent_config_versions"));
        assert!(sql.contains("`stage`"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = ConfigVersionRepo::new(fake.pool());
    let record = bendclaw::storage::ConfigVersionRecord {
        id: "cv-1".into(),
        agent_id: "agent-1".into(),
        version: 1,
        label: "v1".into(),
        stage: "production".into(),
        system_prompt: "You are helpful.".into(),
        display_name: "Bot".into(),
        description: "A bot".into(),
        identity: "assistant".into(),
        soul: "friendly".into(),
        token_limit_total: None,
        token_limit_daily: None,
        llm_config: None,
        notes: "initial".into(),
        created_at: String::new(),
    };
    repo.insert(&record).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn config_version_list_by_agent_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE agent_id = 'agent-1'"));
        assert!(sql.contains("ORDER BY version DESC"));
        Ok(version_response("cv-1", "1"))
    });
    let repo = ConfigVersionRepo::new(fake.pool());
    let versions = repo.list_by_agent("agent-1", 10).await?;
    assert_eq!(versions.len(), 1);
    assert_eq!(versions[0].version, 1);
    Ok(())
}

#[tokio::test]
async fn config_version_get_version_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("agent_id = 'agent-1'"));
        assert!(sql.contains("version = 3"));
        Ok(version_response("cv-3", "3"))
    });
    let repo = ConfigVersionRepo::new(fake.pool());
    let v = repo.get_version("agent-1", 3).await?.expect("should exist");
    assert_eq!(v.version, 3);
    Ok(())
}

#[tokio::test]
async fn config_version_next_version_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("COALESCE(MAX(version), 0)"));
        assert!(sql.contains("agent_id = 'agent-1'"));
        Ok(paged_rows(&[&["5"]], None, None))
    });
    let repo = ConfigVersionRepo::new(fake.pool());
    let next = repo.next_version("agent-1").await?;
    assert_eq!(next, 6);
    Ok(())
}
