use std::sync::Arc;
use std::sync::Mutex;

use bendclaw::channels::ChannelRegistry;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;
use bendclaw::tasks::delivery::delivery_service::deliver_result;

use crate::common::fake_databend::FakeDatabend;

fn sample_task() -> TaskRecord {
    TaskRecord {
        id: "task-1".to_string(),
        node_id: "inst-1".to_string(),
        name: "nightly-report".to_string(),
        prompt: "run report".to_string(),
        enabled: true,
        status: "idle".to_string(),
        schedule: TaskSchedule::Every { seconds: 60 },
        delivery: TaskDelivery::None,
        user_id: String::new(),
        scope: "private".to_string(),
        created_by: String::new(),
        last_error: None,
        delete_after_run: false,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at: None,
        lease_token: None,
        lease_node_id: None,
        lease_expires_at: None,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

fn fake_pool(rows: Vec<Vec<serde_json::Value>>) -> bendclaw::storage::Pool {
    let rows = Arc::new(Mutex::new(rows));
    FakeDatabend::new(move |_sql, _database| {
        Ok(bendclaw::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: rows.lock().expect("rows").clone(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    })
    .pool()
}

#[tokio::test]
async fn reports_missing_channel_account() {
    let registry = ChannelRegistry::new();
    let pool = fake_pool(Vec::new());
    let task = TaskRecord {
        delivery: TaskDelivery::Channel {
            channel_account_id: "missing".to_string(),
            chat_id: "chat-42".to_string(),
        },
        ..sample_task()
    };

    let (status, error) = deliver_result(
        &registry,
        &pool,
        &reqwest::Client::new(),
        &task,
        "ok",
        Some("done"),
        None,
    )
    .await;

    assert_eq!(status.as_deref(), Some("failed"));
    assert!(error
        .as_deref()
        .is_some_and(|value| value.contains("not found")));
}
