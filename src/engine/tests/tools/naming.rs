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
fn the_bash_background_guideline_names_both_collection_paths() {
    // Reading and waiting are both legitimate, and the guideline has to say what
    // each is for rather than rank them. The earlier "only when" phrasing carried
    // over Claude Code's stance that blocking is a misuse.
    use std::sync::Arc;

    use evotengine::tools::BashTool;
    use evotengine::tools::ProcessManager;

    let bash = BashTool::new().with_process_manager(Arc::new(ProcessManager::new()));
    let guidelines = bash.prompt_guidelines();
    let guideline = guidelines.first().copied().unwrap_or_default();

    // Both paths present, each with the situation it serves.
    assert!(guideline.contains("output path"), "got: {guideline}");
    assert!(guideline.contains("check progress"), "got: {guideline}");
    assert!(guideline.contains("wait when"), "got: {guideline}");
    // No language that makes waiting conditional on the reading path failing.
    assert!(!guideline.contains("only when"), "got: {guideline}");
}

#[test]
fn a_bash_without_background_support_offers_no_background_guideline() {
    // Headless and readonly have no task_output tool, so a guideline naming it
    // would point at something absent.
    use evotengine::tools::BashTool;

    assert!(BashTool::new().prompt_guidelines().is_empty());
}

#[test]
fn the_bash_description_resolves_task_stop_per_model() {
    // Descriptions pass through `resolve_tool_refs` (see build_tool_definitions),
    // so a placeholder is correct here and a literal `task_stop` would name a
    // tool Claude models do not have -- they see `TaskStop`.
    use std::sync::Arc;

    use evotengine::tools::BashTool;
    use evotengine::tools::ProcessManager;
    use evotengine::tools::TaskStopTool;

    let manager = Arc::new(ProcessManager::new());
    let tools: Vec<Box<dyn AgentTool>> = vec![
        Box::new(BashTool::new().with_process_manager(manager.clone())),
        Box::new(TaskStopTool::new(manager)),
    ];
    let description = tools[0].description();

    let claude = resolve_tool_refs(description, &tools, "claude-opus-4-6");
    assert!(claude.contains("TaskStop"), "got: {claude}");
    assert!(!claude.contains("{{"), "unresolved placeholder: {claude}");

    let other = resolve_tool_refs(description, &tools, "gpt-4o");
    assert!(other.contains("task_stop"), "got: {other}");
    assert!(!other.contains("{{"), "unresolved placeholder: {other}");
}

#[test]
fn text_that_bypasses_the_resolver_keeps_literal_names() {
    // The schema and the background guidance are handed to the model verbatim:
    // parameters_schema() is passed through untouched by build_tool_definitions,
    // and tool-result bodies never reach the resolver at all. A placeholder in
    // either would render as a raw `{{...}}`.
    use std::sync::Arc;

    use evotengine::tools::BashTool;
    use evotengine::tools::ProcessManager;

    let bash = BashTool::new().with_process_manager(Arc::new(ProcessManager::new()));
    let schema = bash.parameters_schema().to_string();
    assert!(!schema.contains("{{"), "got: {schema}");
    assert!(schema.contains("task_stop"), "got: {schema}");
}

#[test]
fn the_literal_names_in_tool_result_text_still_dispatch() {
    // BACKGROUND_GUIDANCE keeps canonical names because tool-result bodies never
    // pass through the resolver. That is only safe because dispatch accepts any
    // alias regardless of model: a Claude model reads `task_output` there while
    // its own schema says `TaskOutput`, and calling either has to work.
    //
    // If dispatch ever became model-aware, those literal names would turn into
    // dead advice rather than a cosmetic mismatch.
    use std::sync::Arc;

    use evotengine::tools::ProcessManager;
    use evotengine::tools::TaskOutputTool;
    use evotengine::tools::TaskStopTool;

    let manager = Arc::new(ProcessManager::new());
    let output = TaskOutputTool::new(manager.clone());
    let stop = TaskStopTool::new(manager);

    // The name the guidance text uses.
    assert!(output.matches_call_name("task_output"));
    assert!(stop.matches_call_name("task_stop"));
    // The name that model's schema advertises.
    assert!(output.matches_call_name("TaskOutput"));
    assert!(stop.matches_call_name("TaskStop"));
}
