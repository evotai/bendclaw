//! Tests for ToolRegistry — helper for building tool definitions and bindings.

use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::tools::selection::tool_registry::ToolRegistry;
use bendclaw::kernel::tools::OperationClassifier;
use bendclaw::kernel::tools::Tool;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::kernel::tools::ToolId;
use bendclaw::kernel::tools::ToolResult;
use bendclaw::kernel::Impact;
use bendclaw::kernel::OpType;

struct StubTool {
    name: &'static str,
}

impl OperationClassifier for StubTool {
    fn op_type(&self) -> OpType {
        OpType::FileRead
    }
    fn classify_impact(&self, _: &serde_json::Value) -> Option<Impact> {
        None
    }
    fn summarize(&self, _: &serde_json::Value) -> String {
        self.name.to_string()
    }
}

#[async_trait]
impl Tool for StubTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "stub"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute_with_context(
        &self,
        _args: serde_json::Value,
        _ctx: &ToolContext,
    ) -> bendclaw::base::Result<ToolResult> {
        Ok(ToolResult::ok("ok"))
    }
}

fn stub(name: &'static str) -> Arc<dyn Tool> {
    Arc::new(StubTool { name })
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn register_and_get() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("alpha"));
    reg.register(stub("beta"));

    assert!(reg.get("alpha").is_some());
    assert!(reg.get("beta").is_some());
    assert!(reg.get("gamma").is_none());
}

#[test]
fn register_builtin_stores_by_id_name() {
    let mut reg = ToolRegistry::new();
    reg.register_builtin(ToolId::Read, stub("read"));

    assert!(reg.get("read").is_some());
}

#[test]
fn list_returns_sorted_names() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("zebra"));
    reg.register(stub("alpha"));
    reg.register(stub("middle"));

    let names = reg.list();
    assert_eq!(names, vec!["alpha", "middle", "zebra"]);
}

#[test]
fn tool_schemas_returns_sorted_schemas() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("beta"));
    reg.register(stub("alpha"));

    let schemas = reg.tool_schemas();
    assert_eq!(schemas.len(), 2);
    assert_eq!(schemas[0].function.name, "alpha");
    assert_eq!(schemas[1].function.name, "beta");
}

#[test]
fn tool_specs_returns_sorted_specs() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("zz"));
    reg.register(stub("aa"));

    let specs = reg.tool_specs();
    assert_eq!(specs.len(), 2);
    assert_eq!(specs[0].name, "aa");
    assert_eq!(specs[1].name, "zz");
}

#[test]
fn get_by_ids_returns_matching_schemas() {
    let mut reg = ToolRegistry::new();
    reg.register_builtin(ToolId::Read, stub("read"));
    reg.register_builtin(ToolId::Bash, stub("bash"));
    reg.register_builtin(ToolId::Grep, stub("grep"));

    let schemas = reg.get_by_ids(&[ToolId::Read, ToolId::Grep]);
    let names: Vec<&str> = schemas.iter().map(|s| s.function.name.as_str()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"read"));
    assert!(names.contains(&"grep"));
}

#[test]
fn get_by_ids_skips_missing() {
    let mut reg = ToolRegistry::new();
    reg.register_builtin(ToolId::Read, stub("read"));

    let schemas = reg.get_by_ids(&[ToolId::Read, ToolId::Bash]);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].function.name, "read");
}

#[test]
fn get_by_names_returns_matching_schemas() {
    let mut reg = ToolRegistry::new();
    reg.register(stub("read"));
    reg.register(stub("bash"));

    let schemas = reg.get_by_names(&["read", "missing"]);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].function.name, "read");
}

#[test]
fn default_registry_is_empty() {
    let reg = ToolRegistry::default();
    assert!(reg.list().is_empty());
    assert!(reg.tool_schemas().is_empty());
}
