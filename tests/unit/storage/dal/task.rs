use anyhow::Result;
use bendclaw::storage::TaskDelivery;
use bendclaw::storage::TaskHistoryRecord;
use bendclaw::storage::TaskRecord;
use bendclaw::storage::TaskSchedule;

// ── TaskRecord ──

#[test]
fn task_delivery_channel_roundtrip() -> Result<()> {
    let delivery = TaskDelivery::Channel {
        channel_account_id: "channel-1".into(),
        chat_id: "chat-42".into(),
    };
    let json = serde_json::to_string(&delivery)?;
    let parsed: TaskDelivery = serde_json::from_str(&json)?;
    assert_eq!(parsed, delivery);
    Ok(())
}

#[test]
fn task_delivery_validate_rejects_missing_channel_fields() {
    let delivery = TaskDelivery::Channel {
        channel_account_id: String::new(),
        chat_id: "chat-42".into(),
    };
    assert!(delivery.validate().is_err());
}

fn make_task() -> TaskRecord {
    TaskRecord {
        id: "task-001".into(),
        executor_node_id: "os-abc12345".into(),
        name: "Daily report".into(),
        prompt: "Generate daily report".into(),
        enabled: true,
        status: "idle".into(),
        schedule: TaskSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: Some("Asia/Shanghai".into()),
        },
        delivery: TaskDelivery::Webhook {
            url: "https://example.com/hook".into(),
        },
        last_error: None,
        delete_after_run: false,
        run_count: 5,
        last_run_at: "2026-03-08T09:00:00Z".into(),
        next_run_at: Some("2026-03-09T09:00:00Z".into()),
        lease_token: None,
        lease_node_id: None,
        lease_expires_at: None,
        created_at: "2026-01-01T00:00:00Z".into(),
        updated_at: "2026-03-08T09:00:00Z".into(),
    }
}

#[test]
fn task_record_serde_roundtrip() -> Result<()> {
    let record = make_task();
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "task-001");
    assert_eq!(parsed.executor_node_id, "os-abc12345");
    assert_eq!(parsed.name, "Daily report");
    assert_eq!(parsed.prompt, "Generate daily report");
    assert!(parsed.enabled);
    assert_eq!(parsed.status, "idle");
    assert_eq!(parsed.schedule, TaskSchedule::Cron {
        expr: "0 9 * * *".into(),
        tz: Some("Asia/Shanghai".into())
    });
    assert_eq!(parsed.delivery, TaskDelivery::Webhook {
        url: "https://example.com/hook".into()
    });
    assert!(!parsed.delete_after_run);
    assert_eq!(parsed.run_count, 5);
    Ok(())
}

#[test]
fn task_record_schedule_kind_at() -> Result<()> {
    let record = TaskRecord {
        schedule: TaskSchedule::At {
            time: "2026-12-31T23:59:00Z".into(),
        },
        delete_after_run: true,
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.schedule, TaskSchedule::At {
        time: "2026-12-31T23:59:00Z".into()
    });
    assert!(parsed.delete_after_run);
    Ok(())
}

#[test]
fn task_record_schedule_kind_every() -> Result<()> {
    let record = TaskRecord {
        schedule: TaskSchedule::Every { seconds: 300 },
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.schedule, TaskSchedule::Every { seconds: 300 });
    Ok(())
}

#[test]
fn task_record_optional_fields_none() -> Result<()> {
    let record = TaskRecord {
        schedule: TaskSchedule::Every { seconds: 300 },
        delivery: TaskDelivery::None,
        last_error: None,
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.schedule, TaskSchedule::Every { seconds: 300 });
    assert_eq!(parsed.delivery, TaskDelivery::None);
    assert!(parsed.last_error.is_none());
    Ok(())
}

#[test]
fn task_record_last_error_present() -> Result<()> {
    let record = TaskRecord {
        status: "error".into(),
        last_error: Some("connection timeout".into()),
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.status, "error");
    assert_eq!(parsed.last_error.as_deref(), Some("connection timeout"));
    Ok(())
}

// ── TaskHistoryRecord ──

fn make_history() -> TaskHistoryRecord {
    TaskHistoryRecord {
        id: "hist-001".into(),
        task_id: "task-001".into(),
        run_id: Some("run-abc".into()),
        task_name: "Daily report".into(),
        schedule: TaskSchedule::Cron {
            expr: "0 9 * * *".into(),
            tz: Some("Asia/Shanghai".into()),
        },
        prompt: "Generate daily report".into(),
        status: "ok".into(),
        output: Some("Report generated successfully".into()),
        error: None,
        duration_ms: Some(1500),
        delivery: TaskDelivery::Webhook {
            url: "https://example.com/hook".into(),
        },
        delivery_status: Some("ok".into()),
        delivery_error: None,
        executed_by_node_id: None,
        created_at: "2026-03-09T09:00:01Z".into(),
    }
}

#[test]
fn task_history_record_serde_roundtrip() -> Result<()> {
    let record = make_history();
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.id, "hist-001");
    assert_eq!(parsed.task_id, "task-001");
    assert_eq!(parsed.run_id.as_deref(), Some("run-abc"));
    assert_eq!(parsed.task_name, "Daily report");
    assert_eq!(parsed.schedule, TaskSchedule::Cron {
        expr: "0 9 * * *".into(),
        tz: Some("Asia/Shanghai".into())
    });
    assert_eq!(parsed.status, "ok");
    assert_eq!(
        parsed.output.as_deref(),
        Some("Report generated successfully")
    );
    assert!(parsed.error.is_none());
    assert_eq!(parsed.duration_ms, Some(1500));
    assert_eq!(parsed.delivery, TaskDelivery::Webhook {
        url: "https://example.com/hook".into()
    });
    assert_eq!(parsed.delivery_status.as_deref(), Some("ok"));
    assert!(parsed.delivery_error.is_none());
    Ok(())
}

#[test]
fn task_history_record_error_status() -> Result<()> {
    let record = TaskHistoryRecord {
        status: "error".into(),
        output: None,
        error: Some("LLM rate limit exceeded".into()),
        delivery_status: Some("skipped".into()),
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.status, "error");
    assert!(parsed.output.is_none());
    assert_eq!(parsed.error.as_deref(), Some("LLM rate limit exceeded"));
    assert_eq!(parsed.delivery_status.as_deref(), Some("skipped"));
    Ok(())
}

#[test]
fn task_history_record_delivery_failed() -> Result<()> {
    let record = TaskHistoryRecord {
        delivery_status: Some("failed".into()),
        delivery_error: Some("HTTP 503".into()),
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.delivery_status.as_deref(), Some("failed"));
    assert_eq!(parsed.delivery_error.as_deref(), Some("HTTP 503"));
    Ok(())
}

#[test]
fn task_history_record_no_delivery() -> Result<()> {
    let record = TaskHistoryRecord {
        delivery: TaskDelivery::None,
        delivery_status: None,
        delivery_error: None,
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.delivery, TaskDelivery::None);
    assert!(parsed.delivery_status.is_none());
    assert!(parsed.delivery_error.is_none());
    Ok(())
}

#[test]
fn task_history_record_no_run_id() -> Result<()> {
    let record = TaskHistoryRecord {
        run_id: None,
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert!(parsed.run_id.is_none());
    Ok(())
}

#[test]
fn task_history_record_skipped_status() -> Result<()> {
    let record = TaskHistoryRecord {
        status: "skipped".into(),
        output: None,
        error: None,
        duration_ms: Some(0),
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.status, "skipped");
    assert_eq!(parsed.duration_ms, Some(0));
    Ok(())
}
