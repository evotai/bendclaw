use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::scheduler::executor::compute_next_run;
use chrono::NaiveDateTime;
use chrono::Utc;

// ── schedule_kind = "every" ──

#[test]
fn compute_next_run_every_with_seconds() -> Result<()> {
    let before = Utc::now();
    let result = compute_next_run("every", "", Some(300));
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(
        result.as_deref().context("expected Some")?,
        "%Y-%m-%d %H:%M:%S",
    )?;
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 299 && diff.num_seconds() <= 301);
    Ok(())
}

#[test]
fn compute_next_run_every_defaults_to_60() -> Result<()> {
    let before = Utc::now();
    let result = compute_next_run("every", "", None);
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(
        result.as_deref().context("expected Some")?,
        "%Y-%m-%d %H:%M:%S",
    )?;
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 59 && diff.num_seconds() <= 61);
    Ok(())
}

#[test]
fn compute_next_run_every_custom_interval() -> Result<()> {
    let before = Utc::now();
    let result = compute_next_run("every", "", Some(15));
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(
        result.as_deref().context("expected Some")?,
        "%Y-%m-%d %H:%M:%S",
    )?;
    let diff = ts.and_utc() - before;
    assert!(diff.num_seconds() >= 14 && diff.num_seconds() <= 16);
    Ok(())
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
fn compute_next_run_cron_returns_timestamp() -> Result<()> {
    let result = compute_next_run("cron", "0 0 9 * * *", None);
    assert!(result.is_some());
    let ts = NaiveDateTime::parse_from_str(
        result.as_deref().context("expected Some")?,
        "%Y-%m-%d %H:%M:%S",
    )?;
    assert!(ts.and_utc() > Utc::now());
    Ok(())
}

#[test]
fn compute_next_run_cron_ignores_every_seconds() {
    let result = compute_next_run("cron", "0 0 9 * * *", Some(300));
    assert!(result.is_some());
    assert!(!result.as_deref().is_some_and(|s| s.contains("300")));
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
