use anyhow::Result;
use bendclaw::storage::TaskHistoryRepo;

use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;
use crate::common::task_rows::task_history_query;
use crate::common::task_rows::TaskHistoryRow;

#[tokio::test]
async fn task_history_repo_lists_entries_for_task() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, task_id, run_id, task_name, schedule, prompt, status, output, error, duration_ms, delivery, delivery_status, delivery_error, executed_by_node_id, TO_VARCHAR(created_at) FROM task_history WHERE task_id = 'task-1' ORDER BY created_at DESC LIMIT 10"
        );
        Ok(task_history_query([TaskHistoryRow::ok("task-1")]))
    });
    let repo = TaskHistoryRepo::new(fake.pool());

    let history = repo.list_by_task("task-1", 10).await?;

    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, "hist-1");
    assert_eq!(history[0].task_id, "task-1");
    assert_eq!(history[0].status, "ok");
    assert_eq!(history[0].duration_ms, Some(1200));
    assert_eq!(
            fake.calls(),
            vec![FakeDatabendCall::Query {
            sql: "SELECT id, task_id, run_id, task_name, schedule, prompt, status, output, error, duration_ms, delivery, delivery_status, delivery_error, executed_by_node_id, TO_VARCHAR(created_at) FROM task_history WHERE task_id = 'task-1' ORDER BY created_at DESC LIMIT 10".to_string(),
            database: None,
        }]
    );
    Ok(())
}
