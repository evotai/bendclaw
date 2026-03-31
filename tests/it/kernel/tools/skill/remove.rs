//! Tests for [`SkillRemoveTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::skills::projector::SkillProjector;
use bendclaw::kernel::tools::skill_remove::SkillRemoveTool;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use serde_json::json;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::mocks::context::test_tool_context;

fn make_tool() -> SkillRemoveTool {
    let fake = FakeDatabend::new(|_sql, _database| Ok(paged_rows(&[], None, None)));
    let dir = std::env::temp_dir().join(format!("bendclaw-rm-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let projector = Arc::new(SkillProjector::new(
        dir,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = AgentConfig::default();
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"));
    let meta_pool = fake.pool().with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm));
    SkillRemoveTool::new(org.skills().clone())
}

#[tokio::test]
async fn remove_rejects_path_traversal_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "../evil"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("skill name") || e.contains("invalid owner")));
    Ok(())
}

#[tokio::test]
async fn remove_rejects_empty_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({"name": ""}), &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_single_char_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "a"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_uppercase_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "MySkill"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_reserved_name() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "shell"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

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

#[test]
fn remove_tool_name() {
    let tool = make_tool();
    assert_eq!(tool.name(), "remove_skill");
}

#[test]
fn remove_tool_description() {
    let tool = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn remove_tool_schema_has_name_field() {
    let tool = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["name"].is_object());
}

#[tokio::test]
async fn remove_namespaced_key_dispatches_unsubscribe() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    // owner/name format should go through unsubscribe path, not delete
    // With noop stores this succeeds (no DB to fail against)
    let result = tool
        .execute_with_context(json!({"name": "alice/my-report"}), &ctx)
        .await?;
    assert!(
        result.success,
        "namespaced remove should succeed: {:?}",
        result.error
    );
    assert!(result.output.contains("unsubscribed"));
    Ok(())
}

#[tokio::test]
async fn remove_bare_name_dispatches_delete() -> Result<()> {
    let tool = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "my-report"}), &ctx)
        .await?;
    assert!(
        result.success,
        "bare remove should succeed: {:?}",
        result.error
    );
    assert!(result.output.contains("removed"));
    Ok(())
}

#[test]
fn remove_tool_op_type() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let tool = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
