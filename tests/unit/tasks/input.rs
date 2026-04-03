use anyhow::Result;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskSchedule;
use bendclaw::tasks::input::TaskCreateSpec;
use bendclaw::tasks::input::TaskUpdateSpec;
use bendclaw::tasks::input::TaskUpdateToolInput;

#[test]
fn task_create_spec_defaults_delivery_and_delete_after_run() -> Result<()> {
    let spec: TaskCreateSpec = serde_json::from_value(serde_json::json!({
        "name": "nightly-report",
        "prompt": "run report",
        "schedule": {
            "kind": "every",
            "seconds": 60
        }
    }))?;

    assert_eq!(spec.name, "nightly-report");
    assert_eq!(spec.delivery, TaskDelivery::None);
    assert!(!spec.delete_after_run);
    assert_eq!(spec.schedule, TaskSchedule::Every { seconds: 60 });
    Ok(())
}

#[test]
fn task_create_spec_rejects_unknown_fields() {
    let err = serde_json::from_value::<TaskCreateSpec>(serde_json::json!({
        "name": "nightly-report",
        "prompt": "run report",
        "schedule": {
            "kind": "every",
            "seconds": 60
        },
        "unexpected": true
    }))
    .expect_err("unknown field should fail");

    assert!(err.to_string().contains("unknown field"));
}

#[test]
fn task_update_spec_allows_partial_fields() -> Result<()> {
    let spec: TaskUpdateSpec = serde_json::from_value(serde_json::json!({
        "enabled": false,
        "delivery": {
            "kind": "channel",
            "channel_account_id": "channel-1",
            "chat_id": "chat-42"
        }
    }))?;

    assert_eq!(spec.enabled, Some(false));
    assert_eq!(
        spec.delivery,
        Some(TaskDelivery::Channel {
            channel_account_id: "channel-1".to_string(),
            chat_id: "chat-42".to_string()
        })
    );
    assert!(spec.name.is_none());
    assert!(spec.schedule.is_none());
    Ok(())
}

#[test]
fn task_update_tool_input_parses_task_id_and_spec() -> Result<()> {
    let input: TaskUpdateToolInput = serde_json::from_value(serde_json::json!({
        "task_id": "task-1",
        "name": "updated-report"
    }))?;

    assert_eq!(input.task_id, "task-1");
    assert_eq!(input.spec.name.as_deref(), Some("updated-report"));
    Ok(())
}
