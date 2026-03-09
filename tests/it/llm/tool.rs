use bendclaw::llm::tool::FunctionDef;
use bendclaw::llm::tool::ToolSchema;
use serde_json::json;

#[test]
fn tool_schema_new_sets_type_to_function() {
    let schema = ToolSchema::new("shell", "Run a command", json!({"type": "object"}));
    assert_eq!(schema.schema_type, "function");
    assert_eq!(schema.function.name, "shell");
    assert_eq!(schema.function.description, "Run a command");
}

#[test]
fn tool_schema_serde_roundtrip() {
    let schema = ToolSchema::new(
        "read",
        "Read a file",
        json!({"type": "object", "properties": {}}),
    );
    let json = serde_json::to_string(&schema).unwrap();
    let parsed: ToolSchema = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.schema_type, "function");
    assert_eq!(parsed.function.name, "read");
    assert_eq!(parsed.function.description, "Read a file");
}

#[test]
fn function_def_serde_roundtrip() {
    let def = FunctionDef {
        name: "edit".into(),
        description: "Edit a file".into(),
        parameters: json!({"type": "object"}),
    };
    let json = serde_json::to_string(&def).unwrap();
    let parsed: FunctionDef = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.name, "edit");
    assert_eq!(parsed.description, "Edit a file");
}

#[test]
fn tool_schema_clone() {
    let schema = ToolSchema::new("shell", "desc", json!({}));
    let cloned = schema.clone();
    assert_eq!(cloned.function.name, "shell");
}

#[test]
fn tool_schema_serializes_type_as_type() {
    let schema = ToolSchema::new("test", "desc", json!({}));
    let json = serde_json::to_value(&schema).unwrap();
    assert_eq!(json["type"], "function");
    assert!(json.get("schema_type").is_none());
}
