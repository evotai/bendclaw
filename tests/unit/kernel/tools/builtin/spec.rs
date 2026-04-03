use std::sync::Arc;

use bendclaw::tools::tool_contract::Tool;
use bendclaw::tools::tool_services::NoopSecretUsageSink;

fn assert_valid_spec(tool: &dyn Tool) {
    let spec = tool.spec();
    assert!(!spec.name.is_empty(), "tool name must not be empty");
    assert!(
        !spec.description.is_empty(),
        "tool '{}' description must not be empty",
        spec.name
    );
    let params = spec.parameters;
    assert!(
        params.is_object(),
        "tool '{}' parameters must be a JSON object",
        spec.name
    );
    assert!(
        params.get("type").and_then(|v| v.as_str()) == Some("object"),
        "tool '{}' parameters.type must be 'object'",
        spec.name
    );
    assert!(
        params.get("properties").is_some(),
        "tool '{}' parameters must have 'properties'",
        spec.name
    );
}

#[test]
fn file_read_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::FileReadTool);
}

#[test]
fn file_write_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::FileWriteTool);
}

#[test]
fn file_edit_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::FileEditTool);
}

#[test]
fn list_dir_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::ListDirTool);
}

#[test]
fn glob_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::GlobTool);
}

#[test]
fn grep_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::filesystem::GrepTool);
}

#[test]
fn bash_spec_is_valid() {
    let sink: Arc<dyn bendclaw::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    assert_valid_spec(&bendclaw::tools::shell::ShellTool::new(
        sink,
    ));
}

#[test]
fn web_fetch_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::web::WebFetchTool);
}

#[test]
fn web_search_spec_is_valid() {
    assert_valid_spec(&bendclaw::tools::web::WebSearchTool::default());
}

#[test]
fn all_core_tool_names_are_unique() {
    let sink: Arc<dyn bendclaw::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(bendclaw::tools::filesystem::FileReadTool),
        Box::new(bendclaw::tools::filesystem::FileWriteTool),
        Box::new(bendclaw::tools::filesystem::FileEditTool),
        Box::new(bendclaw::tools::filesystem::ListDirTool),
        Box::new(bendclaw::tools::filesystem::GlobTool),
        Box::new(bendclaw::tools::filesystem::GrepTool),
        Box::new(bendclaw::tools::shell::ShellTool::new(
            sink.clone(),
        )),
        Box::new(bendclaw::tools::web::WebFetchTool),
        Box::new(bendclaw::tools::web::WebSearchTool::new(
            "https://example.com",
            sink,
        )),
    ];
    let mut names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
    let count = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), count, "duplicate tool names detected");
}

#[test]
fn tool_spec_descriptions_contain_usage_guidance() {
    let sink: Arc<dyn bendclaw::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    let tools: Vec<Box<dyn Tool>> = vec![
        Box::new(bendclaw::tools::filesystem::FileReadTool),
        Box::new(bendclaw::tools::shell::ShellTool::new(
            sink.clone(),
        )),
        Box::new(bendclaw::tools::web::WebFetchTool),
        Box::new(bendclaw::tools::web::WebSearchTool::new(
            "https://example.com",
            sink,
        )),
    ];
    for tool in &tools {
        let desc = tool.description();
        assert!(
            desc.len() > 50,
            "tool '{}' description too short ({} chars) — should contain usage guidance",
            tool.name(),
            desc.len()
        );
    }
}
