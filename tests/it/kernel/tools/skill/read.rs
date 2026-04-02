//! Tests for [`SkillReadTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::tools::builtin::skills::skill_read::SkillReadTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use serde_json::json;

use crate::mocks::context::test_tool_context;

fn make_tool() -> (SkillReadTool, Arc<SkillCatalog>) {
    let dir = std::env::temp_dir().join(format!("bendclaw-read-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let catalog = Arc::new(SkillCatalog::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let tool = SkillReadTool::new(catalog.clone());
    (tool, catalog)
}

// ── metadata ──

#[test]
fn skill_read_tool_name() {
    let (tool, _) = make_tool();
    assert_eq!(tool.name(), "skill_read");
}

#[test]
fn skill_read_tool_description() {
    let (tool, _) = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn skill_read_tool_schema_has_path() {
    let (tool, _) = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["path"].is_object());
}

// ── summarize ──

#[test]
fn summarize_returns_path() {
    let (tool, _) = make_tool();
    assert_eq!(tool.summarize(&json!({"path": "cloud-sql"})), "cloud-sql");
}

#[test]
fn summarize_missing_path_returns_unknown() {
    let (tool, _) = make_tool();
    assert_eq!(tool.summarize(&json!({})), "unknown");
}

// ── execute ──

#[tokio::test]
async fn execute_skill_not_found() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"path": "nonexistent"}), &ctx)
        .await?;

    assert!(result.success);
    assert!(result.output.contains("Skill not found"));
    assert!(result.output.contains("nonexistent"));
    Ok(())
}

#[tokio::test]
async fn execute_missing_path_param() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let result = tool.execute_with_context(json!({}), &ctx).await?;

    assert!(result.success);
    assert!(result.output.contains("Skill not found"));
    Ok(())
}
