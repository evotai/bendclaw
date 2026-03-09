use bendclaw::kernel::scheduler::executor::compute_next_run;
use chrono::NaiveDateTime;
use chrono::Utc;

// ── schedule_kind = "every" ──

#[test]
fn compute_next_run_every_with_seconds() {
    let before = Utc::now();
    let result = compute_next_run("every", "", Some(300));
    assert!(result.is_some());
    let ts =
        NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S").unwrap();
    let diff = ts.and_utc() - before;
    // Should be ~300 seconds in the future (allow 1s tolerance)
    assert!(diff.num_seconds() >= 299 && diff.num_seconds() <= 301);
}

#[test]
fn compute_next_run_every_defaults_to_60() {
    let before = Utc::now();
    let result = compute_next_run("every", "", None);
    assert!(result.is_some());
    let ts =
        NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S").unwrap();
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 59 && diff.num_seconds() <= 61);
}

#[test]
fn compute_next_run_every_custom_interval() {
    let before = Utc::now();
    let result = compute_next_run("every", "", Some(15));
    assert!(result.is_some());
    let ts =
        NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S").unwrap();
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 14 && diff.num_seconds() <= 16);
}

// ── schedule_kind = "at" ──

#[test]
fn compute_next_run_at_returns_none() {
    let result = compute_next_run("at", "", None);
    assert!(result.is_none());
}

#[test]
fn compute_next_run_at_ignores_every_seconds() {
    let result = compute_next_run("at", "", Some(300));
    assert!(result.is_none());
}

// ── schedule_kind = "cron" ──

#[test]
fn compute_next_run_cron_returns_timestamp() {
    // "0 0 9 * * *" = every day at 09:00:00 (cron crate uses 6-field format)
    let result = compute_next_run("cron", "0 0 9 * * *", None);
    assert!(result.is_some());
    let ts =
        NaiveDateTime::parse_from_str(result.as_deref().unwrap(), "%Y-%m-%d %H:%M:%S").unwrap();
    assert!(ts.and_utc() > Utc::now());
}

#[test]
fn compute_next_run_cron_ignores_every_seconds() {
    let result = compute_next_run("cron", "0 0 9 * * *", Some(300));
    assert!(result.is_some());
    // Should be a timestamp, not contain "300"
    assert!(!result.as_deref().unwrap().contains("300"));
}

#[test]
fn compute_next_run_cron_invalid_expr_returns_none() {
    let result = compute_next_run("cron", "not-a-cron", None);
    assert!(result.is_none());
}

#[test]
fn compute_next_run_cron_empty_expr_returns_none() {
    let result = compute_next_run("cron", "", None);
    assert!(result.is_none());
}

// ── unknown schedule_kind ──

#[test]
fn compute_next_run_unknown_kind_returns_none() {
    let result = compute_next_run("unknown", "", None);
    assert!(result.is_none());
}

#[test]
fn compute_next_run_empty_kind_returns_none() {
    let result = compute_next_run("", "", None);
    assert!(result.is_none());
}
