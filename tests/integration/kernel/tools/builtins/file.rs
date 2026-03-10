use bendclaw::kernel::tools::file::FileEditTool;
use bendclaw::kernel::tools::file::FileReadTool;
use bendclaw::kernel::tools::file::FileWriteTool;
use bendclaw::kernel::tools::Tool;
use bendclaw_test_harness::mocks::context::dummy_pool;
use bendclaw_test_harness::mocks::context::test_workspace;
use serde_json::json;

fn make_ctx(workspace_dir: std::path::PathBuf) -> bendclaw::kernel::tools::ToolContext {
    use ulid::Ulid;
    bendclaw::kernel::tools::ToolContext {
        user_id: format!("u-{}", Ulid::new()).into(),
        session_id: format!("s-{}", Ulid::new()).into(),
        agent_id: "a1".into(),
        workspace: test_workspace(workspace_dir),
        pool: dummy_pool(),
    }
}

// ── FileReadTool ──

#[tokio::test]
async fn file_read_success() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("main.rs"), "fn main() {}")?;
    let tool = FileReadTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"path": "main.rs"}), &ctx)
        .await?;
    assert!(result.success);
    assert_eq!(result.output, "fn main() {}");
    Ok(())
}

#[tokio::test]
async fn file_read_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileReadTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"path": "nonexistent.rs"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Failed to read") || e.contains("Path escapes")));
    Ok(())
}

#[tokio::test]
async fn file_read_missing_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileReadTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing")));
    Ok(())
}

// ── FileWriteTool ──

#[tokio::test]
async fn file_write_success() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileWriteTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"path": "out.txt", "content": "hello world"}), &ctx)
        .await?;
    assert!(result.success);
    assert!(result.output.contains("11 bytes"));

    let content = std::fs::read_to_string(dir.path().join("out.txt"))?;
    assert_eq!(content, "hello world");
    Ok(())
}

#[tokio::test]
async fn file_write_missing_content() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileWriteTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"path": "out.txt"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing")));
    Ok(())
}

#[tokio::test]
async fn file_write_missing_path() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileWriteTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(json!({"content": "data"}), &ctx)
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("Missing")));
    Ok(())
}

// ── FileEditTool ──

#[tokio::test]
async fn file_edit_success() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("app.rs"), "fn old() {}")?;
    let tool = FileEditTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(
            json!({"path": "app.rs", "old_string": "old", "new_string": "new"}),
            &ctx,
        )
        .await?;
    assert!(result.success);
    assert!(result.output.contains("Edited"));

    let content = std::fs::read_to_string(dir.path().join("app.rs"))?;
    assert_eq!(content, "fn new() {}");
    Ok(())
}

#[tokio::test]
async fn file_edit_old_string_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("app.rs"), "fn main() {}")?;
    let tool = FileEditTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(
            json!({"path": "app.rs", "old_string": "nonexistent", "new_string": "x"}),
            &ctx,
        )
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("not found")));
    Ok(())
}

#[tokio::test]
async fn file_edit_ambiguous_match() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("app.rs"), "aaa aaa")?;
    let tool = FileEditTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(
            json!({"path": "app.rs", "old_string": "aaa", "new_string": "bbb"}),
            &ctx,
        )
        .await?;
    assert!(!result.success);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|e| e.contains("2 times")));
    Ok(())
}

#[tokio::test]
async fn file_edit_file_not_found() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileEditTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let result = tool
        .execute_with_context(
            json!({"path": "missing.rs", "old_string": "a", "new_string": "b"}),
            &ctx,
        )
        .await?;
    assert!(!result.success);
    Ok(())
}

#[tokio::test]
async fn file_edit_missing_params() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let tool = FileEditTool;
    let ctx = make_ctx(dir.path().to_path_buf());

    let r1 = tool.execute_with_context(json!({}), &ctx).await?;
    assert!(!r1.success);

    let r2 = tool
        .execute_with_context(json!({"path": "a.rs"}), &ctx)
        .await?;
    assert!(!r2.success);

    let r3 = tool
        .execute_with_context(json!({"path": "a.rs", "old_string": "x"}), &ctx)
        .await?;
    assert!(!r3.success);
    Ok(())
}

// ── Tool names ──

#[test]
fn file_tool_names() {
    assert_eq!(FileReadTool.name(), "file_read");
    assert_eq!(FileWriteTool.name(), "file_write");
    assert_eq!(FileEditTool.name(), "file_edit");
}

// ── OperationClassifier: op_type ──

#[test]
fn file_tool_op_types() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::OpType;
    assert_eq!(FileReadTool.op_type(), OpType::FileRead);
    assert_eq!(FileWriteTool.op_type(), OpType::FileWrite);
    assert_eq!(FileEditTool.op_type(), OpType::Edit);
}

// ── OperationClassifier: classify_impact ──

#[test]
fn file_read_classify_impact_is_none() {
    use bendclaw::kernel::tools::OperationClassifier;
    // FileReadTool uses the default impl → None
    assert_eq!(FileReadTool.classify_impact(&serde_json::json!({})), None);
}

#[test]
fn file_write_classify_impact_is_medium() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        FileWriteTool.classify_impact(&serde_json::json!({})),
        Some(Impact::Medium)
    );
}

#[test]
fn file_edit_classify_impact_is_medium() {
    use bendclaw::kernel::tools::OperationClassifier;
    use bendclaw::kernel::Impact;
    assert_eq!(
        FileEditTool.classify_impact(&serde_json::json!({})),
        Some(Impact::Medium)
    );
}

// ── OperationClassifier: summarize ──

#[test]
fn file_read_summarize_returns_path() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(
        FileReadTool.summarize(&serde_json::json!({"path": "src/main.rs"})),
        "src/main.rs"
    );
}

#[test]
fn file_read_summarize_missing_path_returns_empty() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(FileReadTool.summarize(&serde_json::json!({})), "");
}

#[test]
fn file_write_summarize_returns_path() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(
        FileWriteTool.summarize(&serde_json::json!({"path": "out.txt", "content": "hi"})),
        "out.txt"
    );
}

#[test]
fn file_write_summarize_missing_path_returns_empty() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(
        FileWriteTool.summarize(&serde_json::json!({"content": "hi"})),
        ""
    );
}

#[test]
fn file_edit_summarize_returns_path() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(
        FileEditTool.summarize(
            &serde_json::json!({"path": "app.rs", "old_string": "a", "new_string": "b"})
        ),
        "app.rs"
    );
}

#[test]
fn file_edit_summarize_missing_path_returns_empty() {
    use bendclaw::kernel::tools::OperationClassifier;
    assert_eq!(
        FileEditTool.summarize(&serde_json::json!({"old_string": "a", "new_string": "b"})),
        ""
    );
}

#[test]
fn file_read_description_not_empty() {
    use bendclaw::kernel::tools::Tool;
    assert!(!FileReadTool.description().is_empty());
}

#[test]
fn file_read_schema_has_path() {
    use bendclaw::kernel::tools::Tool;
    assert!(FileReadTool.parameters_schema()["properties"]["path"].is_object());
}

#[test]
fn file_write_description_not_empty() {
    use bendclaw::kernel::tools::Tool;
    assert!(!FileWriteTool.description().is_empty());
}

#[test]
fn file_write_schema_has_path() {
    use bendclaw::kernel::tools::Tool;
    assert!(FileWriteTool.parameters_schema()["properties"]["path"].is_object());
}

#[test]
fn file_edit_description_not_empty() {
    use bendclaw::kernel::tools::Tool;
    assert!(!FileEditTool.description().is_empty());
}

#[test]
fn file_edit_schema_has_path() {
    use bendclaw::kernel::tools::Tool;
    assert!(FileEditTool.parameters_schema()["properties"]["path"].is_object());
}
