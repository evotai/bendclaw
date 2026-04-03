use anyhow::Result;
use bendclaw::storage::TaskSchedule;
use chrono::NaiveDateTime;
use chrono::Utc;

// ── TaskSchedule::Every ──

#[test]
fn schedule_every_with_seconds() -> Result<()> {
    let before = Utc::now();
    let schedule = TaskSchedule::Every { seconds: 300 };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 299 && diff.num_seconds() <= 301);
    Ok(())
}

#[test]
fn schedule_every_custom_interval() -> Result<()> {
    let before = Utc::now();
    let schedule = TaskSchedule::Every { seconds: 15 };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 14 && diff.num_seconds() <= 16);
    Ok(())
}

// ── TaskSchedule::At ──

#[test]
fn schedule_at_next_run_returns_none() {
    let schedule = TaskSchedule::At {
        time: "2026-12-31T23:59:00Z".into(),
    };
    assert!(schedule.next_run_at().is_none());
}

#[test]
fn schedule_at_initial_returns_time() {
    let schedule = TaskSchedule::At {
        time: "2026-12-31T23:59:00Z".into(),
    };
    assert_eq!(
        schedule.initial_next_run_at().as_deref(),
        Some("2026-12-31T23:59:00Z")
    );
}

// ── TaskSchedule::Cron ──

#[test]
fn schedule_cron_returns_timestamp() -> Result<()> {
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: None,
    };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    assert!(ts.and_utc() > Utc::now());
    Ok(())
}

#[test]
fn schedule_cron_invalid_expr_returns_none() {
    let schedule = TaskSchedule::Cron {
        expr: "not-a-cron".into(),
        tz: None,
    };
    assert!(schedule.next_run_at().is_none());
}

#[test]
fn schedule_cron_empty_expr_returns_none() {
    let schedule = TaskSchedule::Cron {
        expr: "".into(),
        tz: None,
    };
    assert!(schedule.next_run_at().is_none());
}

// ── TaskSchedule::Cron with timezone ──

#[test]
fn schedule_cron_with_timezone_converts_to_utc() -> Result<()> {
    // "0 0 9 * * *" in Asia/Shanghai should produce 01:00 UTC (Shanghai = UTC+8)
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: Some("Asia/Shanghai".into()),
    };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    assert_eq!(ts.and_utc().format("%H:%M").to_string(), "01:00");
    Ok(())
}

#[test]
fn schedule_cron_with_invalid_tz_falls_back_to_utc() -> Result<()> {
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: Some("Fake/Zone".into()),
    };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    assert_eq!(ts.and_utc().format("%H:%M").to_string(), "09:00");
    Ok(())
}

// ── Cron 5-field auto-pad ──

#[test]
fn schedule_cron_five_field_auto_pads() -> Result<()> {
    let schedule = TaskSchedule::Cron {
        expr: "0 9 * * *".into(),
        tz: None,
    };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    assert_eq!(ts.and_utc().format("%H:%M").to_string(), "09:00");
    Ok(())
}

#[test]
fn validate_cron_five_field_ok() {
    let schedule = TaskSchedule::Cron {
        expr: "0 9 * * *".into(),
        tz: None,
    };
    assert!(schedule.validate().is_ok());
}

#[test]
fn schedule_cron_five_field_with_tz() -> Result<()> {
    let schedule = TaskSchedule::Cron {
        expr: "0 9 * * *".into(),
        tz: Some("Asia/Shanghai".into()),
    };
    let result = schedule.next_run_at();
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S")?;
    assert_eq!(ts.and_utc().format("%H:%M").to_string(), "01:00");
    Ok(())
}

// ── validate ──

#[test]
fn validate_cron_ok() {
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: None,
    };
    assert!(schedule.validate().is_ok());
}

#[test]
fn validate_cron_empty_err() {
    let schedule = TaskSchedule::Cron {
        expr: "".into(),
        tz: None,
    };
    assert!(schedule.validate().is_err());
}

#[test]
fn validate_cron_invalid_err() {
    let schedule = TaskSchedule::Cron {
        expr: "not-a-cron".into(),
        tz: None,
    };
    assert!(schedule.validate().is_err());
}

#[test]
fn validate_cron_valid_tz_ok() {
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: Some("America/New_York".into()),
    };
    assert!(schedule.validate().is_ok());
}

#[test]
fn validate_cron_invalid_tz_err() {
    let schedule = TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: Some("Mars/Olympus".into()),
    };
    let err = schedule.validate().unwrap_err();
    assert!(err.contains("unknown timezone"));
}

#[test]
fn validate_every_ok() {
    let schedule = TaskSchedule::Every { seconds: 60 };
    assert!(schedule.validate().is_ok());
}

#[test]
fn validate_every_zero_err() {
    let schedule = TaskSchedule::Every { seconds: 0 };
    assert!(schedule.validate().is_err());
}

#[test]
fn validate_at_ok() {
    let schedule = TaskSchedule::At {
        time: "2026-12-31T23:59:00Z".into(),
    };
    assert!(schedule.validate().is_ok());
}

#[test]
fn validate_at_empty_err() {
    let schedule = TaskSchedule::At { time: "".into() };
    assert!(schedule.validate().is_err());
}

// ── serde ──

#[test]
fn schedule_serde_cron() {
    let json = r#"{"kind":"cron","expr":"0 0 9 * * *","tz":"UTC"}"#;
    let s: TaskSchedule = serde_json::from_str(json).expect("cron schedule");
    assert_eq!(s, TaskSchedule::Cron {
        expr: "0 0 9 * * *".into(),
        tz: Some("UTC".into())
    });
}

#[test]
fn schedule_serde_every() {
    let json = r#"{"kind":"every","seconds":300}"#;
    let s: TaskSchedule = serde_json::from_str(json).expect("every schedule");
    assert_eq!(s, TaskSchedule::Every { seconds: 300 });
}

#[test]
fn schedule_serde_at() {
    let json = r#"{"kind":"at","time":"2026-12-31T23:59:00Z"}"#;
    let s: TaskSchedule = serde_json::from_str(json).expect("at schedule");
    assert_eq!(s, TaskSchedule::At {
        time: "2026-12-31T23:59:00Z".into()
    });
}

#[test]
fn schedule_serde_unknown_kind_errors() {
    let json = r#"{"kind":"unknown"}"#;
    let err = serde_json::from_str::<TaskSchedule>(json).expect_err("unknown kind");
    assert!(err.to_string().contains("unknown variant"));
}

// ── storage expr ──

#[test]
fn to_storage_expr_contains_serialized_schedule() {
    let expr = TaskSchedule::Every { seconds: 60 }
        .to_storage_expr()
        .expect("schedule expr");
    assert!(expr.contains("PARSE_JSON"));
    assert!(expr.contains("\"kind\":\"every\""));
    assert!(expr.contains("\"seconds\":60"));
}
