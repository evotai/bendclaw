use anyhow::Result;
use bendclaw::storage::TaskHistoryRecord;
use bendclaw::storage::TaskRecord;

// ── TaskRecord ──

fn make_task() -> TaskRecord {
    TaskRecord {
        id: "task-001".into(),
        executor_instance_id: "os-abc12345".into(),
        name: "Daily report".into(),
        cron_expr: "0 9 * * *".into(),
        prompt: "Generate daily report".into(),
        enabled: true,
        status: "idle".into(),
        schedule_kind: "cron".into(),
        every_seconds: None,
        at_time: None,
        tz: Some("Asia/Shanghai".into()),
        webhook_url: Some("https://example.com/hook".into()),
        last_error: None,
        delete_after_run: false,
        run_count: 5,
        last_run_at: "2026-03-08T09:00:00Z".into(),
        next_run_at: Some("2026-03-09T09:00:00Z".into()),
        lease_token: None,
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
    assert_eq!(parsed.executor_instance_id, "os-abc12345");
    assert_eq!(parsed.name, "Daily report");
    assert_eq!(parsed.cron_expr, "0 9 * * *");
    assert_eq!(parsed.prompt, "Generate daily report");
    assert!(parsed.enabled);
    assert_eq!(parsed.status, "idle");
    assert_eq!(parsed.schedule_kind, "cron");
    assert_eq!(parsed.tz.as_deref(), Some("Asia/Shanghai"));
    assert_eq!(
        parsed.webhook_url.as_deref(),
        Some("https://example.com/hook")
    );
    assert!(!parsed.delete_after_run);
    assert_eq!(parsed.run_count, 5);
    Ok(())
}

#[test]
fn task_record_schedule_kind_at() -> Result<()> {
    let record = TaskRecord {
        schedule_kind: "at".into(),
        at_time: Some("2026-12-31T23:59:00Z".into()),
        delete_after_run: true,
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.schedule_kind, "at");
    assert_eq!(parsed.at_time.as_deref(), Some("2026-12-31T23:59:00Z"));
    assert!(parsed.delete_after_run);
    Ok(())
}

#[test]
fn task_record_schedule_kind_every() -> Result<()> {
    let record = TaskRecord {
        schedule_kind: "every".into(),
        every_seconds: Some(300),
        cron_expr: String::new(),
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.schedule_kind, "every");
    assert_eq!(parsed.every_seconds, Some(300));
    assert!(parsed.cron_expr.is_empty());
    Ok(())
}

#[test]
fn task_record_optional_fields_none() -> Result<()> {
    let record = TaskRecord {
        every_seconds: None,
        at_time: None,
        tz: None,
        webhook_url: None,
        last_error: None,
        ..make_task()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskRecord = serde_json::from_str(&json)?;
    assert!(parsed.every_seconds.is_none());
    assert!(parsed.at_time.is_none());
    assert!(parsed.tz.is_none());
    assert!(parsed.webhook_url.is_none());
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
        schedule_kind: "cron".into(),
        cron_expr: Some("0 9 * * *".into()),
        prompt: "Generate daily report".into(),
        status: "ok".into(),
        output: Some("Report generated successfully".into()),
        error: None,
        duration_ms: Some(1500),
        webhook_url: Some("https://example.com/hook".into()),
        webhook_status: Some("ok".into()),
        webhook_error: None,
        executed_by_instance_id: None,
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
    assert_eq!(parsed.schedule_kind, "cron");
    assert_eq!(parsed.cron_expr.as_deref(), Some("0 9 * * *"));
    assert_eq!(parsed.status, "ok");
    assert_eq!(
        parsed.output.as_deref(),
        Some("Report generated successfully")
    );
    assert!(parsed.error.is_none());
    assert_eq!(parsed.duration_ms, Some(1500));
    assert_eq!(parsed.webhook_status.as_deref(), Some("ok"));
    assert!(parsed.webhook_error.is_none());
    Ok(())
}

#[test]
fn task_history_record_error_status() -> Result<()> {
    let record = TaskHistoryRecord {
        status: "error".into(),
        output: None,
        error: Some("LLM rate limit exceeded".into()),
        webhook_status: Some("skipped".into()),
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.status, "error");
    assert!(parsed.output.is_none());
    assert_eq!(parsed.error.as_deref(), Some("LLM rate limit exceeded"));
    assert_eq!(parsed.webhook_status.as_deref(), Some("skipped"));
    Ok(())
}

#[test]
fn task_history_record_webhook_failed() -> Result<()> {
    let record = TaskHistoryRecord {
        webhook_status: Some("failed".into()),
        webhook_error: Some("HTTP 503".into()),
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert_eq!(parsed.webhook_status.as_deref(), Some("failed"));
    assert_eq!(parsed.webhook_error.as_deref(), Some("HTTP 503"));
    Ok(())
}

#[test]
fn task_history_record_no_webhook() -> Result<()> {
    let record = TaskHistoryRecord {
        webhook_url: None,
        webhook_status: None,
        webhook_error: None,
        ..make_history()
    };
    let json = serde_json::to_string(&record)?;
    let parsed: TaskHistoryRecord = serde_json::from_str(&json)?;
    assert!(parsed.webhook_url.is_none());
    assert!(parsed.webhook_status.is_none());
    assert!(parsed.webhook_error.is_none());
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
