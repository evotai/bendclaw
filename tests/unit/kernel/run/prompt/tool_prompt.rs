use bendclaw::kernel::run::prompt::tool_prompt::render_tools_section;
use bendclaw::kernel::tools::definition::tool_definition::ToolDefinition;

#[test]
fn render_tools_section_empty_produces_nothing() {
    let mut prompt = String::new();
    render_tools_section(&mut prompt, &[]);
    assert!(prompt.is_empty());
}

#[test]
fn render_tools_section_lists_all_tools() {
    let defs = vec![
        ToolDefinition::from_skill("read".into(), "Read a file".into(), serde_json::json!({})),
        ToolDefinition::from_skill("write".into(), "Write a file".into(), serde_json::json!({})),
    ];
    let mut prompt = String::new();
    render_tools_section(&mut prompt, &defs);
    assert!(prompt.contains("## Available Tools"));
    assert!(prompt.contains("- `read`: Read a file"));
    assert!(prompt.contains("- `write`: Write a file"));
    assert!(prompt.contains("Call tools when they would help"));
}

#[test]
fn render_tools_section_includes_builtin_and_skill_uniformly() {
    let defs = vec![
        ToolDefinition::from_skill(
            "builtin-tool".into(),
            "A builtin".into(),
            serde_json::json!({}),
        ),
        ToolDefinition::from_skill("my-skill".into(), "A skill".into(), serde_json::json!({})),
    ];
    let mut prompt = String::new();
    render_tools_section(&mut prompt, &defs);
    assert!(prompt.contains("- `builtin-tool`: A builtin"));
    assert!(prompt.contains("- `my-skill`: A skill"));
}
