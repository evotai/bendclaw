use anyhow::Result;
use bendclaw::kernel::tools::ToolResult;
use bendclaw::kernel::tools::ToolSpec;

#[test]
fn tool_result_ok_sets_success_true() {
    let r = ToolResult::ok("output");
    assert!(r.success);
}

#[test]
fn tool_result_ok_sets_output() {
    let r = ToolResult::ok("hello");
    assert_eq!(r.output, "hello");
}

#[test]
fn tool_result_ok_error_is_none() {
    let r = ToolResult::ok("ok");
    assert!(r.error.is_none());
}

#[test]
fn tool_result_error_sets_success_false() {
    let r = ToolResult::error("bad");
    assert!(!r.success);
}

#[test]
fn tool_result_error_sets_error_field() {
    let r = ToolResult::error("something went wrong");
    assert_eq!(r.error.as_deref(), Some("something went wrong"));
}

#[test]
fn tool_result_error_output_is_empty() {
    let r = ToolResult::error("err");
    assert!(r.output.is_empty());
}

#[test]
fn tool_result_serde_roundtrip_ok() -> Result<()> {
    let r = ToolResult::ok("result text");
    let json = serde_json::to_string(&r)?;
    let back: ToolResult = serde_json::from_str(&json)?;
    assert!(back.success);
    assert_eq!(back.output, "result text");
    assert!(back.error.is_none());
    Ok(())
}

#[test]
fn tool_result_serde_roundtrip_error() -> Result<()> {
    let r = ToolResult::error("failed");
    let json = serde_json::to_string(&r)?;
    let back: ToolResult = serde_json::from_str(&json)?;
    assert!(!back.success);
    assert!(back.output.is_empty());
    assert_eq!(back.error.as_deref(), Some("failed"));
    Ok(())
}

#[test]
fn tool_spec_serde_roundtrip() -> Result<()> {
    let spec = ToolSpec {
        name: "my_tool".into(),
        description: "does stuff".into(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    };
    let json = serde_json::to_string(&spec)?;
    let back: ToolSpec = serde_json::from_str(&json)?;
    assert_eq!(back.name, "my_tool");
    assert_eq!(back.description, "does stuff");
    Ok(())
}

#[test]
fn tool_spec_fields_accessible() {
    let spec = ToolSpec {
        name: "shell".into(),
        description: "run shell commands".into(),
        parameters: serde_json::json!({"type": "object"}),
    };
    assert_eq!(spec.name, "shell");
    assert_eq!(spec.description, "run shell commands");
    assert_eq!(spec.parameters["type"], "object");
}
