use anyhow::Result;
use async_trait::async_trait;
use bendclaw::tools::Impact;
use bendclaw::tools::OpType;
use bendclaw::tools::OperationClassifier;
use bendclaw::tools::Tool;
use bendclaw::tools::ToolContext;
use bendclaw::tools::ToolResult;
use bendclaw::tools::ToolSpec;
use bendclaw::types::Result as BaseResult;
use serde_json::json;

use crate::mocks::context::test_tool_context;

// ── mock Tool for spec() testing ──

struct EchoTool;

impl OperationClassifier for EchoTool {
    fn op_type(&self) -> OpType {
        OpType::Execute
    }
    fn classify_impact(&self, _args: &serde_json::Value) -> Option<Impact> {
        None
    }
    fn summarize(&self, _args: &serde_json::Value) -> String {
        "echo".into()
    }
}

#[async_trait]
impl Tool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }
    fn description(&self) -> &str {
        "echoes input"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {"msg": {"type": "string"}}})
    }
    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> BaseResult<ToolResult> {
        Ok(ToolResult::ok("echo"))
    }
}

// ── ToolContext ──

#[test]
fn tool_context_fields() {
    let ctx = test_tool_context();
    assert!(!ctx.user_id.is_empty());
    assert!(!ctx.session_id.is_empty());
    assert_eq!(ctx.agent_id.as_ref(), "a1");
}

#[test]
fn tool_context_clone() {
    let ctx = test_tool_context();
    let cloned = ctx.clone();
    assert_eq!(ctx.agent_id, cloned.agent_id);
    assert_eq!(ctx.user_id, cloned.user_id);
}

// ── Tool::spec() default method ──

#[test]
fn tool_spec_from_trait() {
    let tool = EchoTool;
    let spec: ToolSpec = tool.spec();
    assert_eq!(spec.name, "echo");
    assert_eq!(spec.description, "echoes input");
    assert!(spec.parameters.get("properties").is_some());
}

#[test]
fn tool_spec_serde_roundtrip() -> Result<()> {
    let tool = EchoTool;
    let spec = tool.spec();
    let json = serde_json::to_string(&spec)?;
    let back: ToolSpec = serde_json::from_str(&json)?;
    assert_eq!(back.name, "echo");
    assert_eq!(back.description, "echoes input");
    Ok(())
}

#[test]
fn tool_result_ok() {
    let r = ToolResult::ok("done");
    assert!(r.success);
    assert_eq!(r.output, "done");
    assert!(r.error.is_none());
}

#[test]
fn tool_result_error() {
    let r = ToolResult::error("bad input");
    assert!(!r.success);
    assert!(r.output.is_empty());
    assert_eq!(r.error.as_deref(), Some("bad input"));
}

#[test]
fn tool_result_serde_roundtrip_ok() -> Result<()> {
    let r = ToolResult::ok("output");
    let json = serde_json::to_string(&r)?;
    let back: ToolResult = serde_json::from_str(&json)?;
    assert!(back.success);
    assert_eq!(back.output, "output");
    Ok(())
}

#[test]
fn tool_result_serde_roundtrip_error() -> Result<()> {
    let r = ToolResult::error("fail");
    let json = serde_json::to_string(&r)?;
    let back: ToolResult = serde_json::from_str(&json)?;
    assert!(!back.success);
    assert_eq!(back.error.as_deref(), Some("fail"));
    Ok(())
}
