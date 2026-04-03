use std::sync::Arc;

use bendclaw::llm::tool::ToolSchema;
use bendclaw::planning::tool_view::ExpansionStrategy;
use bendclaw::planning::tool_view::ProgressiveToolView;

fn test_tools() -> Arc<Vec<ToolSchema>> {
    Arc::new(vec![
        ToolSchema::new("bash", "Execute shell commands", serde_json::json!({})),
        ToolSchema::new("read", "Read file contents", serde_json::json!({})),
        ToolSchema::new("write", "Write file contents", serde_json::json!({})),
        ToolSchema::new("memory_write", "Write to memory", serde_json::json!({})),
    ])
}

#[test]
fn first_turn_sends_all_tools() {
    let view = ProgressiveToolView::new(test_tools());
    assert_eq!(view.strategy(), ExpansionStrategy::SendAll);
    assert_eq!(view.tool_schemas().len(), 4);
}

#[test]
fn advance_switches_to_expanded() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked("bash");
    view.advance();
    assert_eq!(view.strategy(), ExpansionStrategy::SendExpanded);
    let schemas = view.tool_schemas();
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].function.name, "bash");
}

#[test]
fn expanded_fallback_when_empty() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.advance();
    assert_eq!(view.tool_schemas().len(), 4);
}

#[test]
fn note_invoked_ignores_unknown() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked("nonexistent_tool");
    assert_eq!(view.expanded_count(), 0);
}

#[test]
fn note_invoked_batch() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked_batch(&["bash".into(), "read".into()]);
    view.advance();
    assert_eq!(view.expanded_count(), 2);
    let schemas = view.tool_schemas();
    assert_eq!(schemas.len(), 2);
}

#[test]
fn duplicate_invocations_are_idempotent() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked("bash");
    view.note_invoked("bash");
    assert_eq!(view.expanded_count(), 1);
}

#[test]
fn reset_clears_state() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked("bash");
    view.advance();
    assert_eq!(view.strategy(), ExpansionStrategy::SendExpanded);
    assert_eq!(view.expanded_count(), 1);

    view.reset();
    assert_eq!(view.strategy(), ExpansionStrategy::SendAll);
    assert_eq!(view.expanded_count(), 0);
    assert_eq!(view.tool_schemas().len(), 4);
}

#[test]
fn expanded_names_sorted() {
    let mut view = ProgressiveToolView::new(test_tools());
    view.note_invoked("write");
    view.note_invoked("bash");
    view.note_invoked("read");
    let names = view.expanded_names();
    assert_eq!(names, vec!["bash", "read", "write"]);
}

#[test]
fn total_count() {
    let view = ProgressiveToolView::new(test_tools());
    assert_eq!(view.total_count(), 4);
}
