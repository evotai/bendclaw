use bendclaw::kernel::lease::LeaseResource;
use bendclaw::kernel::task::lease::TaskLeaseResource;
use bendclaw::storage::pool::QueryResponse;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::test_runtime::test_runtime;

fn noop_fake() -> FakeDatabend {
    FakeDatabend::new(|_sql, _db| Ok(paged_rows(&[], None, None)))
}

#[test]
fn claim_condition_includes_stuck_task_recovery() {
    let runtime = test_runtime(noop_fake());
    let resource = TaskLeaseResource::new(runtime, reqwest::Client::new());
    let cond = resource
        .claim_condition()
        .expect("should have claim_condition");
    assert!(cond.contains("enabled = true"), "must require enabled");
    assert!(cond.contains("next_run_at <= NOW()"), "must require due");
    assert!(cond.contains("status != 'running'"), "must exclude running");
    assert!(
        cond.contains("lease_expires_at IS NULL OR lease_expires_at <= NOW()"),
        "must recover stuck running tasks with expired leases"
    );
}

#[test]
fn safe_to_release_reflects_activity_tracker() {
    let runtime = test_runtime(noop_fake());
    let resource = TaskLeaseResource::new(runtime.clone(), reqwest::Client::new());

    assert!(resource.safe_to_release(), "no active tasks → safe");

    let guard = runtime.track_task();
    assert!(!resource.safe_to_release(), "active task → not safe");

    drop(guard);
    assert!(resource.safe_to_release(), "task finished → safe again");
}

#[tokio::test]
async fn discover_returns_empty_for_no_agents() {
    let runtime = test_runtime(noop_fake());
    let resource = TaskLeaseResource::new(runtime, reqwest::Client::new());

    let entries = resource.discover().await.unwrap();
    assert!(entries.is_empty());
}

#[tokio::test]
async fn on_released_does_not_panic_for_missing_task() {
    let runtime = test_runtime(noop_fake());
    let pool = runtime.databases().root_pool().clone();
    let resource = TaskLeaseResource::new(runtime, reqwest::Client::new());

    // Best-effort reset — should not panic.
    resource.on_released("nonexistent-task", &pool).await;
}

#[tokio::test]
async fn discover_carries_agent_id_in_context() {
    use crate::common::task_rows::TaskRow;

    let fake = FakeDatabend::new(|sql, _db| {
        if sql.starts_with("SHOW DATABASES") {
            return Ok(paged_rows(&[&["test_agent1"]], None, None));
        }
        // list_active query
        if sql.contains("WHERE") && sql.contains("next_run_at") {
            return Ok(crate::common::task_rows::task_query([TaskRow::every(
                "task-1", "report", true,
            )]));
        }
        Ok(paged_rows(&[], None, None))
    });

    let runtime = test_runtime(fake);
    let resource = TaskLeaseResource::new(runtime, reqwest::Client::new());

    let entries = resource.discover().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, "task-1");
    assert_eq!(entries[0].context, "agent1", "context must carry agent_id");
}
