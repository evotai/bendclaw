//! Tests for is_concurrency_safe declarations on tools.

use bendengine::tools::*;
use bendengine::types::AgentTool;

#[test]
fn read_only_tools_are_concurrency_safe() {
    let tools: Vec<Box<dyn AgentTool>> = vec![
        Box::new(ReadFileTool::new()),
        Box::new(ListFilesTool::new()),
        Box::new(SearchTool::new()),
        Box::new(WebFetchTool::new()),
    ];
    for tool in &tools {
        assert!(
            tool.is_concurrency_safe(),
            "{} should be concurrency safe",
            tool.name()
        );
    }
}

#[test]
fn mutating_tools_are_not_concurrency_safe() {
    let tools: Vec<Box<dyn AgentTool>> = vec![
        Box::new(EditFileTool::new()),
        Box::new(WriteFileTool::new()),
        Box::new(BashTool::new()),
    ];
    for tool in &tools {
        assert!(
            !tool.is_concurrency_safe(),
            "{} should NOT be concurrency safe",
            tool.name()
        );
    }
}
