use anyhow::Result;
use bendclaw::storage::TaskRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;
use crate::common::task_rows::task_query;
use crate::common::task_rows::TaskRow;

#[tokio::test]
async fn task_repo_list_due_scopes_by_executor_instance() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, executor_node_id, name, prompt, enabled, status, schedule, delivery, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, lease_node_id, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE enabled = true AND status != 'running' AND next_run_at <= NOW() AND executor_node_id = 'inst-1' ORDER BY next_run_at ASC LIMIT 100"
        );
        Ok(task_query([TaskRow::every(
            "task-1",
            "nightly-report",
            true,
        )]))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.list_due("inst-1").await?;

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task-1");
    Ok(())
}

#[tokio::test]
async fn task_repo_claim_due_tasks_updates_then_loads_claimed_rows() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        if sql.starts_with("UPDATE tasks SET status = 'running'") {
            assert!(sql.contains("lease_token = 'lease-1'"));
            assert!(sql.contains("executor_node_id = 'inst-1'"));
            assert!(
                sql.contains("(lease_token IS NULL OR lease_token = '')"),
                "claim_due_tasks must guard against double-claim: {sql}"
            );
            return Ok(paged_rows(&[], None, None));
        }
        assert_eq!(
            sql,
            "SELECT id, executor_node_id, name, prompt, enabled, status, schedule, delivery, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, lease_node_id, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM tasks WHERE lease_token = 'lease-1' AND status = 'running' ORDER BY next_run_at ASC LIMIT 100"
        );
        Ok(task_query([TaskRow {
            lease_token: Some("lease-1".to_string()),
            ..TaskRow::every("task-1", "nightly-report", true)
        }]))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.claim_due_tasks("inst-1", "lease-1").await?;

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].lease_token.as_deref(), Some("lease-1"));
    Ok(())
}

#[tokio::test]
async fn task_repo_finish_task_clears_lease_and_updates_status() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(sql.starts_with("UPDATE tasks SET status = 'completed'"));
        assert!(sql.contains("lease_token = NULL"));
        assert!(sql.contains("last_error = NULL"));
        assert!(sql.contains("next_run_at = NULL"));
        assert!(sql.contains("WHERE id = 'task-1' AND lease_token = 'lease-1'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());

    repo.finish_task("task-1", "lease-1", "completed", None, None)
        .await?;

    assert_eq!(
        fake.calls(),
        vec![FakeDatabendCall::Query {
            sql: "UPDATE tasks SET status = 'completed', lease_token = NULL, lease_node_id = NULL, lease_expires_at = NULL, last_run_at = NOW(), run_count = run_count + 1, updated_at = NOW(), last_error = NULL, next_run_at = NULL WHERE id = 'task-1' AND lease_token = 'lease-1'".to_string(),
            database: None,
        }]
    );
    Ok(())
}

#[tokio::test]
async fn task_repo_toggle_and_delete_issue_expected_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql == "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = 'task-1'"
                || sql == "DELETE FROM tasks WHERE id = 'task-1'"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());

    repo.toggle("task-1").await?;
    repo.delete("task-1").await?;

    assert_eq!(fake.calls(), vec![
        FakeDatabendCall::Query {
            sql: "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = 'task-1'"
                .to_string(),
            database: None,
        },
        FakeDatabendCall::Query {
            sql: "DELETE FROM tasks WHERE id = 'task-1'".to_string(),
            database: None,
        },
    ]);
    Ok(())
}

#[tokio::test]
async fn task_repo_list_due_any_omits_executor_instance_filter() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql.contains("WHERE enabled = true AND next_run_at <= NOW()"),
            "should have base conditions: {sql}"
        );
        assert!(
            !sql.contains("AND executor_node_id ="),
            "list_due_any must not filter by executor_node_id: {sql}"
        );
        assert!(
            sql.contains("status != 'running'"),
            "should filter non-running tasks: {sql}"
        );
        assert!(
            sql.contains("lease_expires_at"),
            "should recover stuck running tasks with expired leases: {sql}"
        );
        Ok(task_query([TaskRow::every("task-1", "report", true)]))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.list_due_any().await?;

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, "task-1");
    Ok(())
}

#[tokio::test]
async fn task_repo_set_status_running_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(sql.starts_with("UPDATE tasks SET"));
        assert!(sql.contains("status = 'running'"));
        assert!(sql.contains("WHERE id = 'task-1'"));
        assert!(sql.contains("status != 'running'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());
    repo.set_status_running("task-1").await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn task_repo_reset_status_if_running_generates_valid_sql() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(sql.starts_with("UPDATE tasks SET"));
        assert!(sql.contains("status = 'idle'"));
        assert!(sql.contains("WHERE id = 'task-1'"));
        assert!(sql.contains("status = 'running'"));
        Ok(paged_rows(&[], None, None))
    });
    let repo = TaskRepo::new(fake.pool());
    repo.reset_status_if_running("task-1").await?;
    assert_eq!(fake.calls().len(), 1);
    Ok(())
}

#[tokio::test]
async fn task_repo_list_active_includes_running_tasks_with_valid_lease() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        // Running tasks with valid leases must be included regardless of enabled.
        assert!(
            sql.contains("status = 'running' AND lease_token IS NOT NULL"),
            "list_active must include running tasks with valid leases: {sql}"
        );
        // Claimable tasks still require enabled = true.
        assert!(
            sql.contains("enabled = true AND next_run_at <= NOW()"),
            "list_active must include due claimable tasks: {sql}"
        );
        // The running-with-lease branch must NOT require enabled = true,
        // so disabling a task mid-execution doesn't cause stale eviction.
        // Verify the OR structure: (enabled = true AND ...) OR (status = 'running' AND lease_token ...)
        let running_branch_pos = sql.find("OR (status = 'running' AND lease_token").unwrap();
        let enabled_before_or = &sql[..running_branch_pos];
        let running_branch = &sql[running_branch_pos..];
        assert!(
            !running_branch.contains("enabled = true"),
            "running-with-lease branch must not filter by enabled: {sql}"
        );
        assert!(
            enabled_before_or.contains("enabled = true"),
            "claimable branch must filter by enabled: {sql}"
        );
        Ok(task_query([TaskRow::every("task-1", "report", true)]))
    });
    let repo = TaskRepo::new(fake.pool());

    let tasks = repo.list_active().await?;

    assert_eq!(tasks.len(), 1);
    Ok(())
}
