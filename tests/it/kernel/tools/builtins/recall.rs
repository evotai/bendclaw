use std::sync::Arc;

use bendclaw::kernel::recall::RecallStore;
use bendclaw::kernel::tools::recall::LearningWriteTool;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use serde_json::json;

use crate::common::fake_databend::rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;
use crate::mocks::context::test_tool_context;

fn make_tool() -> (LearningWriteTool, FakeDatabend) {
    let fake = FakeDatabend::new(|_sql, _db| Ok(rows(&[])));
    let store = Arc::new(RecallStore::new(fake.pool()));
    (LearningWriteTool::new(store), fake)
}

#[tokio::test]
async fn learning_write_success() -> Result<(), Box<dyn std::error::Error>> {
    let (tool, fake) = make_tool();
    let mut ctx = test_tool_context();
    let writer = bendclaw::kernel::writer::tool_op::spawn_tool_writer();
    ctx.tool_writer = writer.clone();

    let result = tool
        .execute_with_context(
            json!({
                "kind": "workflow",
                "subject": "repo",
                "title": "Read AGENTS first",
                "content": "Read AGENTS.md before making repo-specific changes.",
                "priority": 7,
                "confidence": 0.9,
                "conditions": {"repo": "bendclaw"},
                "strategy": {"first_step": "read_agents"}
            }),
            &ctx,
        )
        .await?;

    writer.shutdown().await;

    assert!(result.success);
    assert!(result.output.contains("Read AGENTS first"));

    let calls = fake.calls();
    assert!(calls.iter().any(|call| {
        match call {
            FakeDatabendCall::Query { sql, .. } => {
                sql.contains("INSERT INTO learnings") && sql.contains("workflow")
            }
            _ => false,
        }
    }));
    Ok(())
}

#[tokio::test]
async fn learning_write_validates_required_fields() -> Result<(), Box<dyn std::error::Error>> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let missing_kind = tool
        .execute_with_context(
            json!({"subject": "repo", "title": "x", "content": "y"}),
            &ctx,
        )
        .await?;
    let missing_subject = tool
        .execute_with_context(
            json!({"kind": "workflow", "title": "x", "content": "y"}),
            &ctx,
        )
        .await?;

    assert_eq!(missing_kind.error.as_deref(), Some("kind is required"));
    assert_eq!(
        missing_subject.error.as_deref(),
        Some("subject is required")
    );
    Ok(())
}

#[tokio::test]
async fn learning_write_validates_optional_objects_and_ranges(
) -> Result<(), Box<dyn std::error::Error>> {
    let (tool, _) = make_tool();
    let ctx = test_tool_context();

    let invalid_conditions = tool
        .execute_with_context(
            json!({
                "kind": "pattern",
                "subject": "shell",
                "title": "x",
                "content": "y",
                "conditions": ["not", "an", "object"]
            }),
            &ctx,
        )
        .await?;
    let invalid_confidence = tool
        .execute_with_context(
            json!({
                "kind": "pattern",
                "subject": "shell",
                "title": "x",
                "content": "y",
                "confidence": 1.5
            }),
            &ctx,
        )
        .await?;

    assert_eq!(
        invalid_conditions.error.as_deref(),
        Some("conditions must be an object")
    );
    assert_eq!(
        invalid_confidence.error.as_deref(),
        Some("confidence must be between 0 and 1")
    );
    Ok(())
}

#[test]
fn learning_write_metadata_is_stable() {
    let (tool, _) = make_tool();
    assert_eq!(tool.name(), "learning_write");
    assert_eq!(
        tool.summarize(&json!({"title": "Prefer file_edit"})),
        "Prefer file_edit"
    );
    assert_eq!(
        tool.parameters_schema()["required"],
        json!(["kind", "subject", "title", "content"])
    );
}
