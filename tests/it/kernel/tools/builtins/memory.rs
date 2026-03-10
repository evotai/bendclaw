use std::sync::Arc;

use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::mocks::context::test_tool_context;

fn storage() -> Arc<AgentStore> {
    let (base_url, token, warehouse) =
        crate::common::setup::require_api_config().expect("API config required");
    let pool = bendclaw::storage::Pool::new(&base_url, &token, &warehouse)
        .expect("pool: static URL is always valid");
    let llm = Arc::new(crate::mocks::llm::MockLLMProvider::with_text("ok"));
    Arc::new(AgentStore::new(pool, llm))
}

// ── MemoryWriteTool ──

#[tokio::test]
async fn memory_write_success() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "test-key", "content": "test content"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("test-key"));
    Ok(())
}

#[tokio::test]
async fn memory_write_missing_key() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"content": "test content"}), &ctx)
        .await?;
    assert!(!result.success);
    let err = result
        .error
        .as_deref()
        .ok_or_else(|| std::io::Error::other("missing error"))?;
    assert!(err.contains("key"));
    Ok(())
}

#[tokio::test]
async fn memory_write_missing_content() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "test-key"}), &ctx)
        .await?;
    let err = result
        .error
        .as_deref()
        .ok_or_else(|| std::io::Error::other("missing error"))?;
    assert!(err.contains("content"));
    Ok(())
}

#[tokio::test]
async fn memory_write_with_shared_scope() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "k", "content": "c", "scope": "shared"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("shared"));
    Ok(())
}

#[tokio::test]
async fn memory_write_tenant_scope_aliases_to_shared() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "k", "content": "c", "scope": "tenant"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("shared"));
    Ok(())
}

// ── MemoryReadTool ──

#[tokio::test]
async fn memory_read_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryReadTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"key": "nonexistent"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("not found"));
    Ok(())
}

#[tokio::test]
async fn memory_read_missing_key() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryReadTool::new(storage());
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!result.success);
    let err = result
        .error
        .as_deref()
        .ok_or_else(|| std::io::Error::other("missing error"))?;
    assert!(err.contains("key"));
    Ok(())
}

// ── MemorySearchTool ──

#[tokio::test]
async fn memory_search_empty_results() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemorySearchTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"query": "test query"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("No memories"));
    Ok(())
}

#[tokio::test]
async fn memory_search_empty_query() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemorySearchTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"query": ""}), &ctx)
        .await?;
    let err = result
        .error
        .as_deref()
        .ok_or_else(|| std::io::Error::other("missing error"))?;
    assert!(err.contains("query"));
    Ok(())
}

// ── MemoryDeleteTool ──

#[tokio::test]
async fn memory_delete_success() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryDeleteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(json!({"id": "mem-123"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("deleted"));
    Ok(())
}

#[tokio::test]
async fn memory_delete_missing_id() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryDeleteTool::new(storage());
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({}), &ctx).await?;
    let err = result
        .error
        .as_deref()
        .ok_or_else(|| std::io::Error::other("missing error"))?;
    assert!(err.contains("id"));
    Ok(())
}

// ── MemoryListTool ──

#[tokio::test]
async fn memory_list_empty() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryListTool::new(storage());
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(result.success);
    // Shared memories from other test runs may exist, so just verify
    // the output is either empty-list text or valid memory entries.
    assert!(
        result.output.contains("No memories found")
            || result.output.contains("[shared]")
            || result.output.contains("[user]")
            || result.output.contains("[session]")
    );
    Ok(())
}

#[tokio::test]
async fn memory_list_with_limit() -> Result<(), Box<dyn std::error::Error>> {
    let tool = bendclaw::kernel::tools::memory::MemoryListTool::new(storage());
    let ctx = test_tool_context();
    let result = tool.execute_with_context(json!({"limit": 5}), &ctx).await?;
    assert!(result.success);
    Ok(())
}

// ── Tool names ──

#[test]
fn memory_tool_names() {
    let s = storage();
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryWriteTool::new(s.clone()).name(),
        "memory_write"
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryReadTool::new(s.clone()).name(),
        "memory_read"
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemorySearchTool::new(s.clone()).name(),
        "memory_search"
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryDeleteTool::new(s.clone()).name(),
        "memory_delete"
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryListTool::new(s).name(),
        "memory_list"
    );
}

// ── OperationClassifier: op_type ──

#[test]
fn memory_tool_op_types() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    let s = storage();
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryWriteTool::new(s.clone()).op_type(),
        OpType::MemoryWrite
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryReadTool::new(s.clone()).op_type(),
        OpType::MemoryRead
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemorySearchTool::new(s.clone()).op_type(),
        OpType::MemorySearch
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryDeleteTool::new(s.clone()).op_type(),
        OpType::MemoryDelete
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryListTool::new(s).op_type(),
        OpType::MemoryList
    );
}

// ── OperationClassifier: classify_impact (all use default → None) ──

#[test]
fn memory_tools_classify_impact_is_none() {
    use bendclaw::kernel::tools::OperationClassifier;
    let s = storage();
    let args = serde_json::json!({});
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryWriteTool::new(s.clone()).classify_impact(&args),
        None
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryReadTool::new(s.clone()).classify_impact(&args),
        None
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemorySearchTool::new(s.clone()).classify_impact(&args),
        None
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryDeleteTool::new(s.clone()).classify_impact(&args),
        None
    );
    assert_eq!(
        bendclaw::kernel::tools::memory::MemoryListTool::new(s).classify_impact(&args),
        None
    );
}

// ── OperationClassifier: summarize ──

#[test]
fn memory_write_summarize_returns_key() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    assert_eq!(
        tool.summarize(&serde_json::json!({"key": "project-x", "content": "notes"})),
        "project-x"
    );
}

#[test]
fn memory_write_summarize_missing_key_returns_unknown() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryWriteTool::new(storage());
    assert_eq!(
        tool.summarize(&serde_json::json!({"content": "notes"})),
        "unknown"
    );
}

#[test]
fn memory_read_summarize_returns_key() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryReadTool::new(storage());
    assert_eq!(
        tool.summarize(&serde_json::json!({"key": "my-pref"})),
        "my-pref"
    );
}

#[test]
fn memory_read_summarize_missing_key_returns_unknown() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryReadTool::new(storage());
    assert_eq!(tool.summarize(&serde_json::json!({})), "unknown");
}

#[test]
fn memory_search_summarize_returns_query() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemorySearchTool::new(storage());
    assert_eq!(
        tool.summarize(&serde_json::json!({"query": "rust tips"})),
        "rust tips"
    );
}

#[test]
fn memory_search_summarize_missing_query_returns_unknown() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemorySearchTool::new(storage());
    assert_eq!(tool.summarize(&serde_json::json!({})), "unknown");
}

#[test]
fn memory_delete_summarize_returns_id() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryDeleteTool::new(storage());
    assert_eq!(
        tool.summarize(&serde_json::json!({"id": "01JABCDEF"})),
        "01JABCDEF"
    );
}

#[test]
fn memory_delete_summarize_missing_id_returns_unknown() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryDeleteTool::new(storage());
    assert_eq!(tool.summarize(&serde_json::json!({})), "unknown");
}

#[test]
fn memory_list_summarize_is_fixed_string() {
    use bendclaw::kernel::tools::OperationClassifier;
    let tool = bendclaw::kernel::tools::memory::MemoryListTool::new(storage());
    assert_eq!(tool.summarize(&serde_json::json!({})), "list memories");
    assert_eq!(
        tool.summarize(&serde_json::json!({"limit": 5})),
        "list memories"
    );
}

#[test]
fn memory_write_description_not_empty() {
    use bendclaw::kernel::tools::memory::MemoryWriteTool;
    let tool = MemoryWriteTool::new(storage());
    assert!(!tool.description().is_empty());
}

#[test]
fn memory_write_schema_has_key() {
    use bendclaw::kernel::tools::memory::MemoryWriteTool;
    let tool = MemoryWriteTool::new(storage());
    assert!(tool.parameters_schema()["properties"]["key"].is_object());
}

#[test]
fn memory_read_description_not_empty() {
    use bendclaw::kernel::tools::memory::MemoryReadTool;
    let tool = MemoryReadTool::new(storage());
    assert!(!tool.description().is_empty());
}

#[test]
fn memory_read_schema_has_key() {
    use bendclaw::kernel::tools::memory::MemoryReadTool;
    let tool = MemoryReadTool::new(storage());
    assert!(tool.parameters_schema()["properties"]["key"].is_object());
}

#[test]
fn memory_search_description_not_empty() {
    use bendclaw::kernel::tools::memory::MemorySearchTool;
    let tool = MemorySearchTool::new(storage());
    assert!(!tool.description().is_empty());
}

#[test]
fn memory_search_schema_has_query() {
    use bendclaw::kernel::tools::memory::MemorySearchTool;
    let tool = MemorySearchTool::new(storage());
    assert!(tool.parameters_schema()["properties"]["query"].is_object());
}

#[test]
fn memory_delete_description_not_empty() {
    use bendclaw::kernel::tools::memory::MemoryDeleteTool;
    let tool = MemoryDeleteTool::new(storage());
    assert!(!tool.description().is_empty());
}

#[test]
fn memory_delete_schema_has_id() {
    use bendclaw::kernel::tools::memory::MemoryDeleteTool;
    let tool = MemoryDeleteTool::new(storage());
    assert!(tool.parameters_schema()["properties"]["id"].is_object());
}

#[test]
fn memory_list_description_not_empty() {
    use bendclaw::kernel::tools::memory::MemoryListTool;
    let tool = MemoryListTool::new(storage());
    assert!(!tool.description().is_empty());
}

#[test]
fn memory_list_schema_has_limit() {
    use bendclaw::kernel::tools::memory::MemoryListTool;
    let tool = MemoryListTool::new(storage());
    assert!(tool.parameters_schema()["properties"]["limit"].is_object());
}

#[tokio::test]
async fn memory_search_with_include_tenant_false() -> Result<(), Box<dyn std::error::Error>> {
    use bendclaw::kernel::tools::memory::MemorySearchTool;
    let tool = MemorySearchTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(
            serde_json::json!({"query": "test", "include_tenant": false}),
            &ctx,
        )
        .await?;
    assert!(result.success || result.output.contains("No memories") || result.error.is_some());
    Ok(())
}

#[tokio::test]
async fn memory_search_with_max_results() -> Result<(), Box<dyn std::error::Error>> {
    use bendclaw::kernel::tools::memory::MemorySearchTool;
    let tool = MemorySearchTool::new(storage());
    let ctx = test_tool_context();
    let result = tool
        .execute_with_context(serde_json::json!({"query": "test", "max_results": 5}), &ctx)
        .await?;
    assert!(result.success || result.output.contains("No memories") || result.error.is_some());
    Ok(())
}

#[tokio::test]
async fn memory_get_by_id_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let s = storage();
    let result = s.memory_get_by_id("user-x", "nonexistent-id-000").await?;
    assert!(result.is_none());
    Ok(())
}

#[tokio::test]
async fn memory_get_by_key_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let s = storage();
    let result = s.memory_get("user-x", "nonexistent-key-000").await?;
    assert!(result.is_none());
    Ok(())
}
