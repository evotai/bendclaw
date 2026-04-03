use bendclaw::tasks::ops::get_task;
use bendclaw::tasks::ops::list_task_history;
use bendclaw::tasks::ops::list_tasks;

use crate::common::fake_databend::FakeDatabend;

fn empty_fake() -> FakeDatabend {
    FakeDatabend::new(|_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    })
}

#[tokio::test]
async fn get_task_returns_none_when_missing() {
    let fake = empty_fake();
    let pool = fake.pool();
    let result = get_task(&pool, "nonexistent").await;
    assert!(result.is_ok());
    assert!(result.as_ref().ok().and_then(|v| v.as_ref()).is_none());
}

#[tokio::test]
async fn list_tasks_returns_empty() {
    let fake = empty_fake();
    let pool = fake.pool();
    let result = list_tasks(&pool, 10).await;
    assert!(result.is_ok());
    assert!(result.as_ref().ok().map(|v| v.is_empty()).unwrap_or(false));
}

#[tokio::test]
async fn list_task_history_returns_empty() {
    let fake = empty_fake();
    let pool = fake.pool();
    let result = list_task_history(&pool, "task-1", 10).await;
    assert!(result.is_ok());
    assert!(result.as_ref().ok().map(|v| v.is_empty()).unwrap_or(false));
}
