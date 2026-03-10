//! Tests for [`SkillCreateTool`].

use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use bendclaw::kernel::tools::skill::SkillCreateTool;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::context::test_tool_context;
use bendclaw_test_harness::mocks::skill::MockSkillCatalog;
use bendclaw_test_harness::mocks::skill::MockSkillStore;
use serde_json::json;

fn make_tool() -> (SkillCreateTool, Arc<MockSkillStore>) {
    let store = Arc::new(MockSkillStore::new());
    let store_clone = store.clone();
    let factory = Arc::new(FixedStoreFactory(store_clone));
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillCreateTool::new(factory, catalog);
    (tool, store)
}

struct FixedStoreFactory(Arc<MockSkillStore>);

impl bendclaw::kernel::skills::repository::SkillRepositoryFactory for FixedStoreFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<Arc<dyn bendclaw::kernel::skills::repository::SkillRepository>>
    {
        Ok(self.0.clone())
    }
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
// Success cases
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_with_py_script_succeeds() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(valid_args(), &ctx).await?;

    assert!(result.success, "got: {:?}", result.error);
    assert!(result.output.contains("json-to-csv"));

    let skill = store.get_skill("json-to-csv").context("skill not found")?;
    assert_eq!(skill.version, "0.1.0");
    assert!(skill.executable);
    assert_eq!(skill.files.len(), 1);
    assert_eq!(skill.files[0].path, "scripts/run.py");
    assert!(skill.files[0].body.contains("import json"));
    Ok(())
}

#[tokio::test]
async fn create_with_sh_script_succeeds() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let args = json!({
        "name": "deploy-tool",
        "description": "Deploy",
        "content": "body",
        "script_name": "run.sh",
        "script_body": "#!/bin/bash\necho ok"
    });

    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(result.success);
    Ok(())
}

#[tokio::test]
async fn create_with_custom_version_and_timeout() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let args = json!({
        "name": "my-tool",
        "description": "A tool",
        "version": "2.0.0",
        "timeout": 120,
        "content": "body",
        "script_name": "run.py",
        "script_body": "pass"
    });

    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(result.success);

    let skill = store.get_skill("my-tool").context("skill not found")?;
    assert_eq!(skill.version, "2.0.0");
    assert_eq!(skill.timeout, 120);
    Ok(())
}

#[tokio::test]
async fn create_defaults_version_to_0_1_0() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(valid_args(), &ctx).await?;
    assert!(result.success);
    assert_eq!(
        store
            .get_skill("json-to-csv")
            .context("skill not found")?
            .version,
        "0.1.0"
    );
    Ok(())
}

#[tokio::test]
async fn create_defaults_timeout_to_30() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(valid_args(), &ctx).await?;
    assert!(result.success);
    assert_eq!(
        store
            .get_skill("json-to-csv")
            .context("skill not found")?
            .timeout,
        30
    );
    Ok(())
}

#[tokio::test]
async fn create_parses_parameters_from_content() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let args = json!({
        "name": "my-tool",
        "description": "desc",
        "content": "## Parameters\n- `--query` : SQL query (required)\n- `--format` : output format",
        "script_name": "run.py",
        "script_body": "pass"
    });

    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(result.success);

    let skill = store.get_skill("my-tool").context("skill not found")?;
    assert_eq!(skill.parameters.len(), 2);
    assert_eq!(skill.parameters[0].name, "query");
    assert!(skill.parameters[0].required);
    assert_eq!(skill.parameters[1].name, "format");
    assert!(!skill.parameters[1].required);
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Name validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_rejects_path_traversal_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("../evil");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_uppercase_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("MySkill");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_reserved_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("shell");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_empty_name() -> Result<()> {
    let (tool, _) = make_tool();
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
    let (tool, _) = make_tool();
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
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["script_name"] = json!("run.rb");
    let result = tool.execute_with_context(args, &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn create_rejects_js_script() -> Result<()> {
    let (tool, _) = make_tool();
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
    let (tool, _) = make_tool();
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
    let (tool, _) = make_tool();
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
// Store interaction
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn create_does_not_store_on_validation_failure() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    let mut args = valid_args();
    args["name"] = json!("../evil");
    let _ = tool.execute_with_context(args, &ctx).await?;
    assert!(!store.contains("../evil"));
    assert!(!store.contains("evil"));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Factory / store error paths
// ═══════════════════════════════════════════════════════════════════════════════

struct FailingStoreFactory;

impl bendclaw::kernel::skills::repository::SkillRepositoryFactory for FailingStoreFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<Arc<dyn bendclaw::kernel::skills::repository::SkillRepository>>
    {
        Err(bendclaw::base::ErrorCode::internal("store unavailable"))
    }
}

#[tokio::test]
async fn create_returns_error_when_factory_fails() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = bendclaw::kernel::tools::skill::SkillCreateTool::new(
        Arc::new(FailingStoreFactory),
        catalog,
    );
    let ctx = bendclaw_test_harness::mocks::context::test_tool_context();
    let result = tool.execute_with_context(valid_args(), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("failed to access agent store")));
    Ok(())
}

struct FailingSaveStore;

#[async_trait::async_trait]
impl bendclaw::kernel::skills::repository::SkillRepository for FailingSaveStore {
    async fn list(&self) -> bendclaw::base::Result<Vec<bendclaw::kernel::skills::skill::Skill>> {
        Ok(vec![])
    }
    async fn get(
        &self,
        _name: &str,
    ) -> bendclaw::base::Result<Option<bendclaw::kernel::skills::skill::Skill>> {
        Ok(None)
    }
    async fn save(
        &self,
        _skill: &bendclaw::kernel::skills::skill::Skill,
    ) -> bendclaw::base::Result<()> {
        Err(bendclaw::base::ErrorCode::internal("save failed"))
    }
    async fn remove(
        &self,
        _name: &str,
        _agent_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> bendclaw::base::Result<()> {
        Ok(())
    }
    async fn checksums(&self) -> bendclaw::base::Result<std::collections::HashMap<String, String>> {
        Ok(std::collections::HashMap::new())
    }
}

struct FailingSaveFactory;

impl bendclaw::kernel::skills::repository::SkillRepositoryFactory for FailingSaveFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<Arc<dyn bendclaw::kernel::skills::repository::SkillRepository>>
    {
        Ok(Arc::new(FailingSaveStore))
    }
}

#[tokio::test]
async fn create_returns_error_when_save_fails() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool =
        bendclaw::kernel::tools::skill::SkillCreateTool::new(Arc::new(FailingSaveFactory), catalog);
    let ctx = bendclaw_test_harness::mocks::context::test_tool_context();
    let result = tool.execute_with_context(valid_args(), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("failed to save skill")));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// summarize
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn summarize_returns_name() {
    use bendclaw::kernel::tools::OperationClassifier;
    let (tool, _) = make_tool();
    assert_eq!(tool.summarize(&json!({"name": "my-skill"})), "my-skill");
}

#[test]
fn summarize_returns_unknown_when_name_missing() {
    use bendclaw::kernel::tools::OperationClassifier;
    let (tool, _) = make_tool();
    assert_eq!(tool.summarize(&json!({})), "unknown");
}

#[tokio::test]
async fn create_overwrites_existing_skill() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();

    let _ = tool.execute_with_context(valid_args(), &ctx).await?;
    assert_eq!(
        store
            .get_skill("json-to-csv")
            .context("skill not found")?
            .version,
        "0.1.0"
    );

    let mut args = valid_args();
    args["version"] = json!("2.0.0");
    let _ = tool.execute_with_context(args, &ctx).await?;
    assert_eq!(
        store
            .get_skill("json-to-csv")
            .context("skill not found")?
            .version,
        "2.0.0"
    );
    Ok(())
}

// ── Tool trait metadata ──

#[test]
fn create_tool_name() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
    assert_eq!(tool.name(), "create_skill");
}

#[test]
fn create_tool_description() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn create_tool_schema_has_required_fields() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
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
    let (tool, _) = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
