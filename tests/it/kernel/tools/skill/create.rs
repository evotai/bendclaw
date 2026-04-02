//! Tests for [`SkillCreateTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::skills::catalog::SkillCatalog;
use bendclaw::kernel::tools::builtin::skills::create::SkillCreateTool;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use serde_json::json;

use crate::mocks::context::test_tool_context;

fn make_tool() -> SkillCreateTool {
    let pool =
        bendclaw::storage::Pool::new("http://localhost:0", "", "default").expect("dummy pool");
    let dir = std::env::temp_dir().join(format!("bendclaw-create-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let projector = Arc::new(SkillCatalog::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = AgentConfig::default();
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"));
    let meta_pool = pool.with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm));
    SkillCreateTool::new(org.manager().clone())
}

fn valid_args() -> serde_json::Value {
    json!({
        "name": "json-to-csv",
        "description": "Convert JSON to CSV",
        "content": "## Parameters\n- `--input` : Path to JSON file (required)",
        "script_name": "run.py",
        "script_body": "import json, sys\nprint('ok')"
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// Name validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_path_traversal_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("../evil");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_uppercase_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("MySkill");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_reserved_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("shell");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_empty_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// File path validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_md_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.md");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("extension")));
    Ok(())
}
#[tokio::test]
async fn create_rejects_rb_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.rb");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_js_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.js");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Size validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_oversized_content() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["content"] = json!("x".repeat(10 * 1024 + 1));
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("content exceeds")));
    Ok(())
}

#[tokio::test]
async fn create_rejects_oversized_script() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_body"] = json!("x".repeat(50 * 1024 + 1));
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("exceeds")));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// summarize
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn summarize_returns_name() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = make_tool();
    assert_eq!(tool.summarize(&json!({"name": "my-skill"})), "my-skill");
}

#[test]
fn summarize_returns_unknown_when_name_missing() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = make_tool();
    assert_eq!(tool.summarize(&json!({})), "unknown");
}

// ── Tool trait metadata ──

#[test]
fn create_tool_name() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert_eq!(tool.name(), "create_skill");
}

#[test]
fn create_tool_description() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn create_tool_schema_has_required_fields() {
    use bendclaw::kernel::tools::Tool;
    let tool = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["name"].is_object());
    assert!(schema["properties"]["description"].is_object());
    assert!(schema["properties"]["content"].is_object());
    assert!(schema["properties"]["script_name"].is_object());
    assert!(schema["properties"]["script_body"].is_object());
}

#[test]
fn create_tool_op_type() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let tool = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
