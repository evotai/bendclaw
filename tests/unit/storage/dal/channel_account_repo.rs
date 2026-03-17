use anyhow::Result;
use bendclaw::storage::ChannelAccountRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn account_row(id: &str) -> Vec<serde_json::Value> {
    vec![
        id,
        "slack",
        "acc-1",
        "agent-1",
        "user-1",
        "{\"token\":\"abc\"}",
        "1",
        "",
        "",
        "", // lease_node_id, lease_token, lease_expires_at
        "2026-03-11T00:00:00Z",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn account_response(id: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".into(),
        error: None,
        data: vec![account_row(id)],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn channel_account_insert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO channel_accounts"));
        assert!(sql.contains("PARSE_JSON"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = ChannelAccountRepo::new(fake.pool());
    let record = bendclaw::storage::ChannelAccountRecord {
        id: "ca-1".into(),
        channel_type: "slack".into(),
        account_id: "acc-1".into(),
        agent_id: "agent-1".into(),
        user_id: "user-1".into(),
        config: serde_json::json!({"token": "abc"}),
        enabled: true,
        lease_node_id: None,
        lease_token: None,
        lease_expires_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn channel_account_load_and_find_generate_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.contains("WHERE id = 'ca-1' LIMIT 1") {
            return Ok(account_response("ca-1"));
        }
        if sql.contains("WHERE channel_type = 'slack' AND account_id = 'acc-1'") {
            return Ok(account_response("ca-1"));
        }
        panic!("unexpected SQL: {sql}");
    });
    let repo = ChannelAccountRepo::new(fake.pool());

    let loaded = repo.load("ca-1").await?.expect("should exist");
    assert_eq!(loaded.channel_type, "slack");
    assert!(loaded.enabled);

    let found = repo
        .find_by_account("slack", "acc-1")
        .await?
        .expect("should exist");
    assert_eq!(found.id, "ca-1");
    Ok(())
}

#[tokio::test]
async fn channel_account_list_by_agent_and_type_generate_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.contains("WHERE agent_id = 'agent-1'") {
            return Ok(account_response("ca-1"));
        }
        if sql.contains("WHERE channel_type = 'slack'") {
            return Ok(account_response("ca-1"));
        }
        panic!("unexpected SQL: {sql}");
    });
    let repo = ChannelAccountRepo::new(fake.pool());

    let by_agent = repo.list_by_agent("agent-1").await?;
    assert_eq!(by_agent.len(), 1);

    let by_type = repo.list_by_type("slack").await?;
    assert_eq!(by_type.len(), 1);
    Ok(())
}

#[tokio::test]
async fn channel_account_update_enabled_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert_eq!(
            sql,
            "UPDATE channel_accounts SET enabled = 1, updated_at = NOW() WHERE id = 'ca-1'"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = ChannelAccountRepo::new(fake.pool());
    repo.update_enabled("ca-1", true).await?;
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "UPDATE channel_accounts SET enabled = 1, updated_at = NOW() WHERE id = 'ca-1'"
            .to_string(),
        database: None,
    }]);
    Ok(())
}

#[tokio::test]
async fn channel_account_delete_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("DELETE FROM channel_accounts"));
        assert!(sql.contains("WHERE id = 'ca-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = ChannelAccountRepo::new(fake.pool());
    repo.delete("ca-1").await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn channel_account_release_lease_clears_lease_columns() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("UPDATE channel_accounts SET"));
        assert!(sql.contains("lease_node_id = NULL"));
        assert!(sql.contains("lease_token = NULL"));
        assert!(sql.contains("lease_expires_at = NULL"));
        assert!(sql.contains("WHERE id = 'ca-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = ChannelAccountRepo::new(fake.pool());
    repo.release_lease("ca-1").await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}
