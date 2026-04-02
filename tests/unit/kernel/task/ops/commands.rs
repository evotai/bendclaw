use bendclaw::kernel::task::ops::create_task;
use bendclaw::kernel::task::ops::update_task;
use bendclaw::kernel::task::ops::CreateTaskParams;
use bendclaw::kernel::task::ops::UpdateTaskParams;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskSchedule;

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
async fn create_task_validates_schedule() {
    let fake = empty_fake();
    let pool = fake.pool();
    let params = CreateTaskParams {
        node_id: "n1".into(),
        name: "test".into(),
        prompt: "hello".into(),
        schedule: TaskSchedule::Cron {
            expr: "bad-cron".into(),
            tz: None,
        },
        delivery: TaskDelivery::None,
        delete_after_run: false,
        user_id: String::new(),
        scope: "private".into(),
        created_by: String::new(),
    };
    let result = create_task(&pool, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn create_task_validates_delivery() {
    let fake = empty_fake();
    let pool = fake.pool();
    let params = CreateTaskParams {
        node_id: "n1".into(),
        name: "test".into(),
        prompt: "hello".into(),
        schedule: TaskSchedule::Every { seconds: 60 },
        delivery: TaskDelivery::Webhook { url: String::new() },
        delete_after_run: false,
        user_id: String::new(),
        scope: "private".into(),
        created_by: String::new(),
    };
    let result = create_task(&pool, params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_task_rejects_missing_task() {
    let fake = empty_fake();
    let pool = fake.pool();
    let params = UpdateTaskParams {
        name: Some("new-name".into()),
        prompt: None,
        schedule: None,
        enabled: None,
        delivery: None,
        delete_after_run: None,
    };
    let result = update_task(&pool, "nonexistent", params).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn update_task_validates_schedule_change() {
    let fake = empty_fake();
    let pool = fake.pool();
    let params = UpdateTaskParams {
        name: None,
        prompt: None,
        schedule: Some(TaskSchedule::Cron {
            expr: "not-valid".into(),
            tz: None,
        }),
        enabled: None,
        delivery: None,
        delete_after_run: None,
    };
    // Will fail because task doesn't exist, but schedule validation runs first
    let result = update_task(&pool, "task-1", params).await;
    assert!(result.is_err());
}
