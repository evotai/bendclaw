use anyhow::Result;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskHistoryRecord;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;
use bendclaw::tasks::view::TaskHistoryView;
use bendclaw::tasks::view::TaskSummaryView;
use bendclaw::tasks::view::TaskView;

fn sample_task() -> TaskRecord {
    TaskRecord {
        id: "task-1".to_string(),
        node_id: "inst-1".to_string(),
        name: "nightly-report".to_string(),
        prompt: "run report".to_string(),
        enabled: true,
        status: "idle".to_string(),
        schedule: TaskSchedule::Every { seconds: 60 },
        delivery: TaskDelivery::Channel {
            channel_account_id: "channel-1".to_string(),
            chat_id: "chat-42".to_string(),
        },
        user_id: String::new(),
        scope: String::new(),
        created_by: String::new(),
        last_error: None,
        delete_after_run: false,
        run_count: 2,
        last_run_at: "2026-03-10T00:00:00Z".to_string(),
        next_run_at: Some("2026-03-11T00:00:00Z".to_string()),
        lease_token: Some("lease-1".to_string()),
        lease_node_id: None,
        lease_expires_at: None,
        created_at: "2026-03-01T00:00:00Z".to_string(),
        updated_at: "2026-03-10T00:00:00Z".to_string(),
    }
}

fn sample_history() -> TaskHistoryRecord {
    TaskHistoryRecord {
        id: "hist-1".to_string(),
        task_id: "task-1".to_string(),
        run_id: Some("run-1".to_string()),
        task_name: "nightly-report".to_string(),
        schedule: TaskSchedule::Every { seconds: 60 },
        prompt: "run report".to_string(),
        status: "ok".to_string(),
        output: Some("done".to_string()),
        error: None,
        duration_ms: Some(1200),
        delivery: TaskDelivery::None,
        delivery_status: Some("ok".to_string()),
        delivery_error: None,
        user_id: String::new(),
        executed_by_node_id: Some("inst-1".to_string()),
        created_at: "2026-03-10T00:05:00Z".to_string(),
    }
}

#[test]
fn task_view_projects_full_task_record() {
    let view = TaskView::from(sample_task());

    assert_eq!(view.id, "task-1");
    assert_eq!(view.name, "nightly-report");
    assert_eq!(view.schedule, TaskSchedule::Every { seconds: 60 });
    assert_eq!(view.delivery, TaskDelivery::Channel {
        channel_account_id: "channel-1".to_string(),
        chat_id: "chat-42".to_string()
    });
    assert_eq!(view.lease_token.as_deref(), Some("lease-1"));
}

#[test]
fn task_summary_view_keeps_compact_fields() -> Result<()> {
    let view = TaskSummaryView::from(sample_task());
    let json = serde_json::to_value(view)?;

    assert!(json.get("prompt").is_none());
    assert_eq!(json["schedule"]["kind"], "every");
    assert_eq!(json["next_run_at"], "2026-03-11T00:00:00Z");
    Ok(())
}

#[test]
fn task_history_view_projects_history_record() {
    let view = TaskHistoryView::from(sample_history());

    assert_eq!(view.id, "hist-1");
    assert_eq!(view.task_id, "task-1");
    assert_eq!(view.status, "ok");
    assert_eq!(view.duration_ms, Some(1200));
    assert_eq!(view.delivery_status.as_deref(), Some("ok"));
}
