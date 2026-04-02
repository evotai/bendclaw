use bendclaw::kernel::tools::definition::tool_definition::ToolDefinition;
use bendclaw::kernel::tools::definition::tool_target::ToolTarget;
use bendclaw::kernel::OpType;

#[test]
fn from_skill_sets_op_type_and_metadata() {
    let def = ToolDefinition::from_skill(
        "my-skill".into(),
        "Does things".into(),
        serde_json::json!({"type": "object"}),
    );
    assert_eq!(def.name, "my-skill");
    assert_eq!(def.description, "Does things");
    assert!(matches!(def.op_type, OpType::SkillRun));
}

#[test]
fn to_tool_schema_roundtrip() {
    let def = ToolDefinition::from_skill(
        "test-tool".into(),
        "A test tool".into(),
        serde_json::json!({"type": "object", "properties": {"x": {"type": "string"}}}),
    );
    let schema = def.to_tool_schema();
    assert_eq!(schema.function.name, "test-tool");
    assert_eq!(schema.function.description, "A test tool");
    assert_eq!(schema.schema_type, "function");
    assert!(schema.function.parameters["properties"]["x"]["type"]
        .as_str()
        .is_some_and(|s| s == "string"));
}

#[test]
fn skill_target_accessors() {
    let skill = ToolTarget::Skill;
    assert!(skill.is_skill());
    assert!(!skill.is_builtin());
    assert!(skill.as_builtin().is_none());
}

#[test]
fn debug_formats_correctly() {
    let skill = ToolTarget::Skill;
    assert_eq!(format!("{skill:?}"), "Skill");

    let def = ToolDefinition::from_skill("x".into(), "y".into(), serde_json::json!({}));
    let dbg = format!("{def:?}");
    assert!(dbg.contains("x"));
}
