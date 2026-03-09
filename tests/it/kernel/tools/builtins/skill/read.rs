//! Tests for [`SkillReadTool`].

use std::sync::Arc;

use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::skills::skill::SkillScope;
use bendclaw::kernel::skills::skill::SkillSource;
use bendclaw::kernel::tools::skill::SkillReadTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;
use crate::mocks::skill::MockSkillCatalog;

fn make_tool() -> (SkillReadTool, Arc<MockSkillCatalog>) {
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillReadTool::new(catalog.clone());
    (tool, catalog)
}

fn insert_skill(catalog: &MockSkillCatalog, name: &str, content: &str) {
    catalog.insert(&Skill {
        name: name.to_string(),
        version: "0.1.0".to_string(),
        description: "test skill".to_string(),
        scope: SkillScope::Global,
        source: SkillSource::Local,
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: content.to_string(),
        files: vec![],
        requires: None,
    });
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
async fn execute_returns_skill_content() {
    let (tool, catalog) = make_tool();
    insert_skill(&catalog, "my-skill", "## Usage\nRun with --input flag.");
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"path": "my-skill"}), &ctx)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.output.contains("## Usage"));
}

#[tokio::test]
async fn execute_skill_not_found() {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"path": "nonexistent"}), &ctx)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.output.contains("Skill not found"));
    assert!(result.output.contains("nonexistent"));
}

#[tokio::test]
async fn execute_missing_path_param() {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let result = tool.execute_with_context(json!({}), &ctx).await.unwrap();

    assert!(result.success);
    assert!(result.output.contains("Skill not found"));
}

#[tokio::test]
async fn execute_truncates_oversized_content() {
    let (tool, catalog) = make_tool();
    // 64 KiB + 1 byte triggers truncation
    let big_content = "x".repeat(64 * 1024 + 100);
    insert_skill(&catalog, "big-skill", &big_content);
    let ctx = test_tool_context();

    let result = tool
        .execute_with_context(json!({"path": "big-skill"}), &ctx)
        .await
        .unwrap();

    assert!(result.success);
    assert!(result.output.contains("truncated"));
}
