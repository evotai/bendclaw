//! Tests for `{{tool}}` placeholder resolution in tool-facing text.

use evotengine::tools::resolve_tool_refs;
use evotengine::tools::GrepTool;
use evotengine::tools::SearchTool;
use evotengine::types::AgentTool;

fn tools() -> Vec<Box<dyn AgentTool>> {
    vec![Box::new(SearchTool::new()), Box::new(GrepTool::new())]
}

#[test]
fn resolves_to_claude_alias() {
    let t = tools();
    let out = resolve_tool_refs(
        "use {{grep}} or {{semantic_code_search}}",
        &t,
        "claude-opus-4-6",
    );
    assert_eq!(out, "use Grep or SemanticCodeSearch");
}

#[test]
fn resolves_to_canonical_for_non_claude() {
    let t = tools();
    let out = resolve_tool_refs("use {{grep}} or {{semantic_code_search}}", &t, "gpt-4o");
    assert_eq!(out, "use grep or semantic_code_search");
}

#[test]
fn unknown_placeholder_emits_literal_name() {
    let t = tools();
    let out = resolve_tool_refs("call {{nonexistent_tool}} now", &t, "claude-opus-4-6");
    assert_eq!(out, "call nonexistent_tool now");
}

#[test]
fn text_without_placeholders_is_unchanged() {
    let t = tools();
    let s = "plain text with no braces";
    assert_eq!(resolve_tool_refs(s, &t, "claude-opus-4-6"), s);
}

#[test]
fn unterminated_placeholder_is_emitted_verbatim() {
    let t = tools();
    let out = resolve_tool_refs("dangling {{grep", &t, "claude-opus-4-6");
    assert_eq!(out, "dangling {{grep");
}

#[test]
fn whitespace_inside_placeholder_is_trimmed() {
    let t = tools();
    let out = resolve_tool_refs("use {{ grep }}", &t, "claude-opus-4-6");
    assert_eq!(out, "use Grep");
}

#[test]
fn the_bash_background_guideline_resolves_task_output_per_model() {
    // The guideline used to hardcode `task_output`, which names a tool a Claude
    // model does not have: it sees `TaskOutput`. The placeholder has to survive
    // into the rendered guideline, so this asserts on the real text rather than a
    // synthetic string.
    use std::sync::Arc;

    use evotengine::tools::BashTool;
    use evotengine::tools::ProcessManager;
    use evotengine::tools::TaskOutputTool;

    let manager = Arc::new(ProcessManager::new());
    let tools: Vec<Box<dyn AgentTool>> = vec![
        Box::new(BashTool::new().with_process_manager(manager.clone())),
        Box::new(TaskOutputTool::new(manager)),
    ];
    let guidelines = tools[0].prompt_guidelines();
    let guideline = guidelines.first().copied().unwrap_or_default();

    let claude = resolve_tool_refs(guideline, &tools, "claude-opus-4-6");
    assert!(claude.contains("TaskOutput"), "got: {claude}");
    assert!(!claude.contains("{{"), "unresolved placeholder: {claude}");
    assert!(!claude.contains("task_output"), "got: {claude}");

    let other = resolve_tool_refs(guideline, &tools, "gpt-4o");
    assert!(other.contains("task_output"), "got: {other}");
    assert!(!other.contains("{{"), "unresolved placeholder: {other}");
}

#[test]
fn the_bash_background_guideline_ranks_reading_over_blocking() {
    // Must agree with BACKGROUND_GUIDANCE and task_output's own schema, both of
    // which treat blocking as a last resort. Offering them as equals here was
    // the contradiction that let a model reach for the blocking call first.
    use std::sync::Arc;

    use evotengine::tools::BashTool;
    use evotengine::tools::ProcessManager;

    let bash = BashTool::new().with_process_manager(Arc::new(ProcessManager::new()));
    let guidelines = bash.prompt_guidelines();
    let guideline = guidelines.first().copied().unwrap_or_default();

    assert!(guideline.contains("output path"), "got: {guideline}");
    assert!(guideline.contains("only when"), "got: {guideline}");
    // The old wording made them interchangeable.
    assert!(!guideline.contains("or use"), "got: {guideline}");
}

#[test]
fn a_bash_without_background_support_offers_no_background_guideline() {
    // Headless and readonly have no task_output tool, so a guideline naming it
    // would point at something absent.
    use evotengine::tools::BashTool;

    assert!(BashTool::new().prompt_guidelines().is_empty());
}
