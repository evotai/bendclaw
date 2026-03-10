//! Tests for [`SkillRemoveTool`].

use std::sync::Arc;

use anyhow::Result;
use bendclaw::kernel::skills::repository::SkillRepository;
use bendclaw::kernel::skills::skill::Skill;
use bendclaw::kernel::tools::skill::SkillRemoveTool;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::context::test_tool_context;
use bendclaw_test_harness::mocks::skill::MockSkillCatalog;
use bendclaw_test_harness::mocks::skill::MockSkillStore;
use serde_json::json;

fn make_tool() -> (SkillRemoveTool, Arc<MockSkillStore>) {
    let store = Arc::new(MockSkillStore::new());
    let store_clone = store.clone();
    let factory = Arc::new(FixedStoreFactory(store_clone));
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillRemoveTool::new(factory, catalog);
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

fn make_skill(name: &str) -> Skill {
    Skill {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        description: "test".to_string(),
        scope: Default::default(),
        source: Default::default(),
        agent_id: None,
        user_id: None,
        timeout: 30,
        executable: false,
        parameters: vec![],
        content: "body".to_string(),
        files: vec![],
        requires: None,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Success cases
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn remove_existing_skill_succeeds() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    store.save(&make_skill("my-skill")).await?;
    assert!(store.contains("my-skill"));

    let result = tool
        .execute_with_context(json!({"name": "my-skill"}), &ctx)
        .await?;
    assert!(result.success, "got: {:?}", result.error);
    assert!(!store.contains("my-skill"));
    Ok(())
}

#[tokio::test]
async fn remove_nonexistent_skill_succeeds() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "no-such-skill"}), &ctx)
        .await?;
    assert!(result.success);
    Ok(())
}

#[tokio::test]
async fn remove_is_idempotent() -> Result<()> {
    let (tool, store) = make_tool();
    let ctx = test_tool_context();
    store.save(&make_skill("my-skill")).await?;

    let _ = tool
        .execute_with_context(json!({"name": "my-skill"}), &ctx)
        .await?;
    let result = tool
        .execute_with_context(json!({"name": "my-skill"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(!store.contains("my-skill"));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// Validation errors
// ═══════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn remove_rejects_path_traversal_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "../evil"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("skill name")));
    Ok(())
}

#[tokio::test]
async fn remove_rejects_empty_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({"name": ""}), &ctx).await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_single_char_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "a"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_uppercase_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "MySkill"}), &ctx)
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn remove_rejects_reserved_name() -> Result<()> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "shell"}), &ctx)
        .await?;
    assert!(!result.success);
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
async fn remove_returns_error_when_factory_fails() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillRemoveTool::new(Arc::new(FailingStoreFactory), catalog);
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "my-skill"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("failed to access agent store")));
    Ok(())
}

struct FailingRemoveStore;

#[async_trait::async_trait]
impl SkillRepository for FailingRemoveStore {
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
        Ok(())
    }
    async fn remove(
        &self,
        _name: &str,
        _agent_id: Option<&str>,
        _user_id: Option<&str>,
    ) -> bendclaw::base::Result<()> {
        Err(bendclaw::base::ErrorCode::internal("remove failed"))
    }
    async fn checksums(&self) -> bendclaw::base::Result<std::collections::HashMap<String, String>> {
        Ok(std::collections::HashMap::new())
    }
}

struct FailingRemoveFactory;

impl bendclaw::kernel::skills::repository::SkillRepositoryFactory for FailingRemoveFactory {
    fn for_agent(
        &self,
        _agent_id: &str,
    ) -> bendclaw::base::Result<Arc<dyn bendclaw::kernel::skills::repository::SkillRepository>>
    {
        Ok(Arc::new(FailingRemoveStore))
    }
}

#[tokio::test]
async fn remove_returns_error_when_store_remove_fails() -> Result<()> {
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillRemoveTool::new(Arc::new(FailingRemoveFactory), catalog);
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"name": "my-skill"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("failed to remove skill")));
    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════════
// summarize
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn summarize_returns_name() {
    use bendclaw::kernel::tools::OperationClassifier;
    let store = Arc::new(MockSkillStore::new());
    let factory = Arc::new(FixedStoreFactory(store));
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillRemoveTool::new(factory, catalog);
    assert_eq!(tool.summarize(&json!({"name": "my-skill"})), "my-skill");
}

#[test]
fn summarize_returns_unknown_when_name_missing() {
    use bendclaw::kernel::tools::OperationClassifier;
    let store = Arc::new(MockSkillStore::new());
    let factory = Arc::new(FixedStoreFactory(store));
    let catalog = Arc::new(MockSkillCatalog::new());
    let tool = SkillRemoveTool::new(factory, catalog);
    assert_eq!(tool.summarize(&json!({})), "unknown");
}

// ── Tool trait metadata ──

#[test]
fn remove_tool_name() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
    assert_eq!(tool.name(), "remove_skill");
}

#[test]
fn remove_tool_description() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
    assert!(!tool.description().is_empty());
}

#[test]
fn remove_tool_schema_has_name_field() {
    use bendclaw::kernel::tools::Tool;
    let (tool, _) = make_tool();
    let schema = tool.parameters_schema();
    assert!(schema["properties"]["name"].is_object());
}

#[test]
fn remove_tool_op_type() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let (tool, _) = make_tool();
    assert_eq!(tool.op_type(), OpType::SkillRun);
}
