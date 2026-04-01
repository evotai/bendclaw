use std::time::Duration;

use anyhow::Result;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;
use bendclaw::kernel::OperationMeta;
use bendclaw::kernel::OperationTracker;

#[test]
fn meta_new_defaults() {
    let meta = OperationMeta::new(OpType::Execute);
    assert_eq!(meta.op_type, OpType::Execute);
    assert!(meta.impact.is_none());
    assert!(meta.timeout_secs.is_none());
    assert_eq!(meta.duration_ms, 0);
    assert!(meta.summary.is_empty());
}

#[test]
fn meta_serde_roundtrip() -> Result<()> {
    let meta = OperationMeta {
        op_type: OpType::FileRead,
        impact: Some(Impact::Low),
        timeout_secs: Some(30),
        duration_ms: 42,
        summary: "read config".into(),
    };
    let json = serde_json::to_string(&meta)?;
    let back: OperationMeta = serde_json::from_str(&json)?;
    assert_eq!(back.op_type, OpType::FileRead);
    assert_eq!(back.impact, Some(Impact::Low));
    assert_eq!(back.timeout_secs, Some(30));
    assert_eq!(back.duration_ms, 42);
    assert_eq!(back.summary, "read config");
    Ok(())
}

#[test]
fn meta_serde_skips_none_fields() -> Result<()> {
    let meta = OperationMeta::new(OpType::Reasoning);
    let json = serde_json::to_string(&meta)?;
    assert!(!json.contains("impact"));
    assert!(!json.contains("timeout_secs"));
    Ok(())
}

#[test]
fn tracker_finish_computes_duration() {
    let tracker = OperationTracker::new(OpType::Execute);
    std::thread::sleep(Duration::from_millis(10));
    let meta = tracker.finish();
    assert!(meta.duration_ms >= 10);
    assert_eq!(meta.op_type, OpType::Execute);
}

#[test]
fn tracker_builder_chain() {
    let meta = OperationTracker::new(OpType::SkillRun)
        .impact(Impact::High)
        .timeout(Duration::from_secs(60))
        .summary("run python")
        .finish();
    assert_eq!(meta.op_type, OpType::SkillRun);
    assert_eq!(meta.impact, Some(Impact::High));
    assert_eq!(meta.timeout_secs, Some(60));
    assert_eq!(meta.summary, "run python");
}

#[test]
fn tracker_maybe_impact_some() {
    let meta = OperationTracker::new(OpType::Edit)
        .maybe_impact(Some(Impact::Medium))
        .finish();
    assert_eq!(meta.impact, Some(Impact::Medium));
}

#[test]
fn tracker_maybe_impact_none() {
    let meta = OperationTracker::new(OpType::Edit)
        .maybe_impact(None)
        .finish();
    assert!(meta.impact.is_none());
}

#[test]
fn tracker_start_time_is_recent() {
    let tracker = OperationTracker::new(OpType::Reasoning);
    let elapsed = tracker.start_time().elapsed();
    assert!(elapsed.as_secs() < 1);
}

#[test]
fn meta_begin_returns_tracker() {
    let tracker = OperationMeta::begin(OpType::Databend);
    let meta = tracker.finish();
    assert_eq!(meta.op_type, OpType::Databend);
}

// ── Impact Display ──

#[test]
fn impact_display() {
    assert_eq!(format!("{}", Impact::Low), "low");
    assert_eq!(format!("{}", Impact::Medium), "medium");
    assert_eq!(format!("{}", Impact::High), "high");
}

#[test]
fn impact_serde_roundtrip() -> Result<()> {
    for impact in [Impact::Low, Impact::Medium, Impact::High] {
        let json = serde_json::to_string(&impact)?;
        let back: Impact = serde_json::from_str(&json)?;
        assert_eq!(back, impact);
    }
    Ok(())
}

// ── OpType Display ──

#[test]
fn op_type_display() {
    assert_eq!(format!("{}", OpType::Reasoning), "REASONING");
    assert_eq!(format!("{}", OpType::Execute), "EXECUTE");
    assert_eq!(format!("{}", OpType::Edit), "EDIT");
    assert_eq!(format!("{}", OpType::FileRead), "FILE_READ");
    assert_eq!(format!("{}", OpType::FileWrite), "FILE_WRITE");
    assert_eq!(format!("{}", OpType::SkillRun), "SKILL_RUN");
    assert_eq!(format!("{}", OpType::Compaction), "COMPACTION");
    assert_eq!(format!("{}", OpType::Databend), "DATABEND");
}

#[test]
fn op_type_serde_roundtrip() -> Result<()> {
    for op in [
        OpType::Reasoning,
        OpType::Execute,
        OpType::Edit,
        OpType::FileRead,
        OpType::FileWrite,
        OpType::SkillRun,
        OpType::Compaction,
        OpType::Databend,
    ] {
        let json = serde_json::to_string(&op)?;
        let back: OpType = serde_json::from_str(&json)?;
        assert_eq!(back, op);
    }
    Ok(())
}

// ── OperationMeta summary ──

#[test]
fn tracker_summary_string_owned() {
    let meta = OperationTracker::new(OpType::Execute)
        .summary(String::from("hello"))
        .finish();
    assert_eq!(meta.summary, "hello");
}

#[test]
fn tracker_summary_str_ref() {
    let meta = OperationTracker::new(OpType::Execute)
        .summary("world")
        .finish();
    assert_eq!(meta.summary, "world");
}

#[test]
fn meta_default_summary_empty() {
    let meta = OperationMeta::new(OpType::Reasoning);
    assert!(meta.summary.is_empty());
}
