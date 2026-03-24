use anyhow::Result;
use bendclaw::storage::RunRepo;
use bendclaw::storage::RunStatus;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn run_row(id: &str, status: &str) -> bendclaw::storage::pool::QueryResponse {
    bendclaw::storage::pool::QueryResponse {
        id: String::new(),
        state: "Succeeded".to_string(),
        error: None,
        data: vec![vec![
            serde_json::Value::String(id.to_string()),
            serde_json::Value::String("session-1".to_string()),
            serde_json::Value::String("agent-1".to_string()),
            serde_json::Value::String("user-1".to_string()),
            serde_json::Value::String("user_turn".to_string()),
            serde_json::Value::String(String::new()), // parent_run_id
            serde_json::Value::String(String::new()), // node_id
            serde_json::Value::String(status.to_string()),
            serde_json::Value::String("hello".to_string()),
            serde_json::Value::String("done".to_string()),
            serde_json::Value::String(String::new()),
            serde_json::Value::String("{\"duration_ms\":42}".to_string()),
            serde_json::Value::String("END_TURN".to_string()),
            serde_json::Value::String(String::new()), // checkpoint_through_run_id
            serde_json::Value::String("3".to_string()),
            serde_json::Value::String("2026-03-11T00:00:00Z".to_string()),
            serde_json::Value::String("2026-03-11T00:01:00Z".to_string()),
        ]],
        next_uri: None,
        final_uri: None,
        schema: Vec::new(),
    }
}

#[tokio::test]
async fn run_repo_load_and_list_for_session_build_expected_queries() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("SELECT COUNT(*) FROM runs WHERE ") {
            return Ok(paged_rows(&[&["2"]], None, None));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE session_id = 'session-1' AND kind != 'session_checkpoint' AND status = 'COMPLETED' ORDER BY created_at DESC LIMIT 20 OFFSET 0") {
            return Ok(run_row("run-1", "COMPLETED"));
        }
        if sql.starts_with("SELECT id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM runs WHERE id = 'run-1' LIMIT 1") {
            return Ok(run_row("run-1", "COMPLETED"));
        }
        panic!("unexpected SQL: {sql}");
    });
    let repo = RunRepo::new(fake.pool());

    let count = repo
        .count_for_session("session-1", Some("COMPLETED"))
        .await?;
    let listed = repo
        .list_for_session("session-1", Some("COMPLETED"), "DESC", 20, 0)
        .await?;
    let loaded = repo.load("run-1").await?.expect("run should exist");

    assert_eq!(count, 2);
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "run-1");
    assert_eq!(listed[0].kind, "user_turn");
    assert_eq!(loaded.status, "COMPLETED");
    Ok(())
}

#[tokio::test]
async fn run_repo_update_final_and_status_issue_expected_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql == "UPDATE runs SET status = 'COMPLETED', output = 'done', error = '', metrics = '{\"duration_ms\":42}', stop_reason = 'END_TURN', iterations = 3, updated_at = NOW() WHERE id = 'run-1'"
                || sql == "UPDATE runs SET status = 'CANCELLED', updated_at = NOW() WHERE id = 'run-1'"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = RunRepo::new(fake.pool());

    repo.update_final(
        "run-1",
        RunStatus::Completed,
        "done",
        "",
        "{\"duration_ms\":42}",
        "END_TURN",
        3,
    )
    .await?;
    repo.update_status("run-1", RunStatus::Cancelled).await?;

    assert_eq!(
        fake.calls(),
        vec![
            FakeDatabendCall::Query {
                sql: "UPDATE runs SET status = 'COMPLETED', output = 'done', error = '', metrics = '{\"duration_ms\":42}', stop_reason = 'END_TURN', iterations = 3, updated_at = NOW() WHERE id = 'run-1'".to_string(),
                database: None,
            },
            FakeDatabendCall::Query {
                sql: "UPDATE runs SET status = 'CANCELLED', updated_at = NOW() WHERE id = 'run-1'".to_string(),
                database: None,
            },
        ]
    );
    Ok(())
}
