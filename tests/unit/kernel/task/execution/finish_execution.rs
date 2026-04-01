use bendclaw::kernel::task::execution::finish_execution;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;

use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

fn sample_task(schedule_kind: &str, delete_after_run: bool) -> TaskRecord {
    TaskRecord {
        id: "task-1".to_string(),
        node_id: "inst-1".to_string(),
        name: "nightly-report".to_string(),
        prompt: "run report".to_string(),
        enabled: true,
        status: "running".to_string(),
        schedule: match schedule_kind {
            "cron" => TaskSchedule::Cron {
                expr: "0 0 9 * * *".to_string(),
                tz: None,
            },
            "at" => TaskSchedule::At {
                time: "2026-12-31T23:59:00Z".to_string(),
            },
            _ => TaskSchedule::Every { seconds: 60 },
        },
        delivery: TaskDelivery::None,
        user_id: String::new(),
        scope: "private".to_string(),
        created_by: String::new(),
        last_error: None,
        delete_after_run,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at: Some("2026-03-11 00:00:00".to_string()),
        lease_token: Some("lease-1".to_string()),
        lease_node_id: None,
        lease_expires_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn sqls(fake: &FakeDatabend) -> Vec<String> {
    fake.calls()
        .into_iter()
        .filter_map(|c| match c {
            FakeDatabendCall::Query { sql, .. } => Some(sql),
            _ => None,
        })
        .collect()
}

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
async fn writes_history_and_updates_task() {
    let fake = empty_fake();
    let pool = fake.pool();
    let task = sample_task("every", false);

    finish_execution(
        &pool,
        &task,
        "lease-1",
        "inst-1",
        "ok",
        Some("run-1".to_string()),
        Some("done".to_string()),
        None,
        1200,
        Some("ok".to_string()),
        None,
    )
    .await
    .expect("finish execution");

    let sqls = sqls(&fake);
    assert!(sqls
        .iter()
        .any(|sql| sql.starts_with("INSERT INTO task_history ")));
    assert!(sqls
        .iter()
        .any(|sql| sql.starts_with("UPDATE tasks SET status = 'ok'")));
    assert!(!sqls
        .iter()
        .any(|sql| sql == "DELETE FROM tasks WHERE id = 'task-1'"));
}

#[tokio::test]
async fn deletes_one_shot_after_run() {
    let fake = empty_fake();
    let pool = fake.pool();
    let task = sample_task("at", true);

    finish_execution(
        &pool,
        &task,
        "lease-1",
        "inst-1",
        "ok",
        Some("run-1".to_string()),
        Some("done".to_string()),
        None,
        1200,
        None,
        None,
    )
    .await
    .expect("finish execution");

    let sqls = sqls(&fake);
    assert!(sqls
        .iter()
        .any(|sql| sql.starts_with("INSERT INTO task_history ")));
    assert!(sqls
        .iter()
        .any(|sql| sql.starts_with("UPDATE tasks SET status = 'ok'")));
    assert!(sqls
        .iter()
        .any(|sql| sql == "DELETE FROM tasks WHERE id = 'task-1'"));
}
