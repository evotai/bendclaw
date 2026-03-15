use anyhow::Result;
use bendclaw::storage::AgentConfigStore;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn config_row(agent_id: &str) -> Vec<serde_json::Value> {
    vec![
        agent_id,
        "You are helpful.",
        "Bot",
        "A bot",
        "assistant",
        "friendly",
        "",
        "",
        "",
        "2026-03-11T00:00:00Z",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn config_response(agent_id: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".into(),
        error: None,
        data: vec![config_row(agent_id)],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn agent_config_get_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE agent_id = 'a-1' LIMIT 1"));
        Ok(config_response("a-1"))
    });
    let store = AgentConfigStore::new(fake.pool());
    let cfg = store.get("a-1").await?.expect("should exist");
    assert_eq!(cfg.agent_id, "a-1");
    assert_eq!(cfg.system_prompt, "You are helpful.");
    Ok(())
}

#[tokio::test]
async fn agent_config_get_any_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("FROM agent_config"));
        assert!(sql.contains("LIMIT 1"));
        Ok(config_response("a-1"))
    });
    let store = AgentConfigStore::new(fake.pool());
    let cfg = store.get_any().await?.expect("should exist");
    assert_eq!(cfg.display_name, "Bot");
    Ok(())
}

#[tokio::test]
async fn agent_config_upsert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("REPLACE INTO agent_config"));
        assert!(sql.contains("ON (agent_id)"));
        assert!(sql.contains("PARSE_JSON"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let store = AgentConfigStore::new(fake.pool());
    store
        .upsert(
            "a-1",
            Some("You are helpful."),
            Some("Bot"),
            Some("A bot"),
            Some("assistant"),
            Some("friendly"),
            Some(Some(100000)),
            Some(Some(10000)),
            Some("{\"model\":\"gpt-4o\"}"),
        )
        .await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn agent_config_upsert_null_limits_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("REPLACE INTO agent_config"));
        assert!(sql.contains("NULL"));
        Ok(paged_rows(&[], None, None))
    });
    let store = AgentConfigStore::new(fake.pool());
    store
        .upsert("a-1", None, None, None, None, None, None, None, None)
        .await?;
    Ok(())
}

#[tokio::test]
async fn agent_config_get_system_prompt_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("system_prompt"));
        assert!(sql.contains("agent_id = 'a-1'"));
        Ok(paged_rows(&[&["You are helpful."]], None, None))
    });
    let store = AgentConfigStore::new(fake.pool());
    let prompt = store.get_system_prompt("a-1").await?;
    assert_eq!(prompt, "You are helpful.");
    Ok(())
}
