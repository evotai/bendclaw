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

// ── from_record ──

#[test]
fn from_record_cron() {
    let s = TaskSchedule::from_record("cron", "0 0 9 * * *", None, None, Some("UTC"));
    assert_eq!(
        s,
        Some(TaskSchedule::Cron {
            expr: "0 0 9 * * *".into(),
            tz: Some("UTC".into())
        })
    );
}

#[test]
fn from_record_every() {
    let s = TaskSchedule::from_record("every", "", Some(300), None, None);
    assert_eq!(s, Some(TaskSchedule::Every { seconds: 300 }));
}

#[test]
fn from_record_at() {
    let s = TaskSchedule::from_record("at", "", None, Some("2026-12-31T23:59:00Z"), None);
    assert_eq!(
        s,
        Some(TaskSchedule::At {
            time: "2026-12-31T23:59:00Z".into()
        })
    );
}

#[test]
fn from_record_unknown_returns_none() {
    let s = TaskSchedule::from_record("unknown", "", None, None, None);
    assert!(s.is_none());
}

// ── kind_str ──

#[test]
fn kind_str_values() {
    assert_eq!(
        TaskSchedule::Cron {
            expr: "".into(),
            tz: None
        }
        .kind_str(),
        "cron"
    );
    assert_eq!(TaskSchedule::Every { seconds: 1 }.kind_str(), "every");
    assert_eq!(TaskSchedule::At { time: "".into() }.kind_str(), "at");
}
