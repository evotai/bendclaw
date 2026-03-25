use anyhow::Result;
use bendclaw::storage::session::repo::SessionWrite;
use bendclaw::storage::session::SessionRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;

fn session_row(id: &str) -> Vec<serde_json::Value> {
    vec![
        id,
        "agent-1",
        "user-1",
        "My Chat",
        "private",
        "",
        "",
        "",
        "{}",
        "{}",
        "2026-03-11T00:00:00Z",
        "2026-03-11T00:01:00Z",
    ]
    .into_iter()
    .map(|s| serde_json::Value::String(s.to_string()))
    .collect()
}

fn session_response(id: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".into(),
        error: None,
        data: vec![session_row(id)],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn session_repo_upsert_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("REPLACE INTO sessions"));
        assert!(sql.contains("PARSE_JSON"));
        assert!(sql.contains("ON (id)"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = SessionRepo::new(fake.pool());
    repo.upsert(SessionWrite {
        session_id: "s-1".to_string(),
        agent_id: "a-1".to_string(),
        user_id: "u-1".to_string(),
        title: "title".to_string(),
        base_key: String::new(),
        replaced_by_session_id: String::new(),
        reset_reason: String::new(),
        session_state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
    })
    .await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn session_repo_load_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE id = 's-1' LIMIT 1"));
        Ok(session_response("s-1"))
    });
    let repo = SessionRepo::new(fake.pool());
    let session = repo.load("s-1").await?.expect("should exist");
    assert_eq!(session.id, "s-1");
    Ok(())
}

#[tokio::test]
async fn session_repo_count_by_user_search_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.starts_with("SELECT COUNT(*) FROM sessions WHERE"));
        assert!(sql.contains("user_id = 'u-1'"));
        Ok(paged_rows(&[&["3"]], None, None))
    });
    let repo = SessionRepo::new(fake.pool());
    let count = repo.count_by_user_search("u-1", None).await?;
    assert_eq!(count, 3);
    Ok(())
}

#[tokio::test]
async fn session_repo_count_by_user_search_with_query_generates_like() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("user_id = 'u-1'"));
        assert!(sql.contains("LIKE"));
        assert!(sql.contains("hello"));
        Ok(paged_rows(&[&["1"]], None, None))
    });
    let repo = SessionRepo::new(fake.pool());
    let count = repo.count_by_user_search("u-1", Some("hello")).await?;
    assert_eq!(count, 1);
    Ok(())
}

#[tokio::test]
async fn session_repo_list_by_user_search_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("FROM sessions WHERE"));
        assert!(sql.contains("user_id = 'u-1'"));
        assert!(sql.contains("ORDER BY updated_at DESC"));
        assert!(sql.contains("LIMIT 20 OFFSET 0"));
        Ok(session_response("s-1"))
    });
    let repo = SessionRepo::new(fake.pool());
    let sessions = repo
        .list_by_user_search("u-1", None, "updated_at DESC", 20, 0)
        .await?;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].id, "s-1");
    Ok(())
}

#[tokio::test]
async fn session_repo_list_by_user_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _db| {
        assert!(sql.contains("WHERE user_id = 'u-1'"));
        assert!(sql.contains("ORDER BY updated_at DESC"));
        Ok(session_response("s-1"))
    });
    let repo = SessionRepo::new(fake.pool());
    let sessions = repo.list_by_user("u-1", 10).await?;
    assert_eq!(sessions.len(), 1);
    Ok(())
}
