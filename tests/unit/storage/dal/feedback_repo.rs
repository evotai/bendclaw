use anyhow::Result;
use bendclaw::storage::FeedbackRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn feedback_row(id: &str) -> Vec<serde_json::Value> {
    vec![
        id,
        "sess-1",
        "run-1",
        "5",
        "great",
        "2026-03-11T00:00:00Z",
        "2026-03-11T00:00:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

#[tokio::test]
async fn feedback_repo_insert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("INSERT INTO feedback"));
        assert!(sql.contains("NOW()"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = FeedbackRepo::new(fake.pool());
    let record = bendclaw::storage::FeedbackRecord {
        id: "fb-1".into(),
        session_id: "sess-1".into(),
        run_id: "run-1".into(),
        rating: 5,
        comment: "great".into(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn feedback_repo_list_and_get_generate_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        if sql.contains("WHERE id = 'fb-1' LIMIT 1") {
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".into(),
                error: None,
                data: vec![feedback_row("fb-1")],
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }
        if sql.contains("ORDER BY created_at DESC LIMIT 10") {
            return Ok(bendclaw::storage::pool::QueryResponse {
                id: String::new(),
                state: "Succeeded".into(),
                error: None,
                data: vec![feedback_row("fb-1")],
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            });
        }
        panic!("unexpected SQL: {sql}");
    });
    let repo = FeedbackRepo::new(fake.pool());

    let listed = repo.list(10).await?;
    assert_eq!(listed.len(), 1);

    let loaded = repo.get("fb-1").await?.expect("should exist");
    assert_eq!(loaded.rating, 5);
    Ok(())
}

#[tokio::test]
async fn feedback_repo_delete_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert_eq!(sql, "DELETE FROM feedback WHERE id = 'fb-1'");
        Ok(paged_rows(&[], None, None))
    });
    let repo = FeedbackRepo::new(fake.pool());
    repo.delete("fb-1").await?;
    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "DELETE FROM feedback WHERE id = 'fb-1'".to_string(),
        database: None,
    }]);
    Ok(())
}
