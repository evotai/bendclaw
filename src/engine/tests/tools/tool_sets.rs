//! Tests for tool set functions (default_tools, base_tools, readonly_tools).

#[tokio::test]
async fn test_default_tools_complete() {
    let tools = bendengine::tools::default_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"bash"));
    assert!(names.contains(&"edit_file"));
    assert!(names.contains(&"list_files"));
    assert!(names.contains(&"web_fetch"));
}

#[tokio::test]
async fn test_base_tools_complete() {
    let tools = bendengine::tools::base_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 7);
    assert!(names.contains(&"bash"));
}

#[tokio::test]
async fn test_readonly_tools_contains_only_safe_tools() {
    let tools = bendengine::tools::readonly_tools();
    let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    assert_eq!(names.len(), 3);
    assert!(names.contains(&"read_file"));
    assert!(names.contains(&"list_files"));
    assert!(names.contains(&"search"));
    // Must not contain mutating or execution tools
    assert!(!names.contains(&"bash"));
    assert!(!names.contains(&"edit_file"));
    assert!(!names.contains(&"write_file"));
    assert!(!names.contains(&"web_fetch"));
}
