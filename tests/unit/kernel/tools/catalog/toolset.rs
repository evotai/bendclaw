//! Tests for tools/toolset — verifies toolset registers expected tools.

use std::collections::HashSet;
use std::sync::Arc;

use bendclaw::kernel::tools::selection::build_local_toolset;
use bendclaw::kernel::tools::tool_services::NoopSecretUsageSink;

fn sink() -> Arc<dyn bendclaw::kernel::tools::tool_services::SecretUsageSink> {
    Arc::new(NoopSecretUsageSink)
}

fn make_local_toolset() -> bendclaw::kernel::tools::definition::toolset::Toolset {
    build_local_toolset(None, sink())
}

#[test]
fn core_catalog_registers_file_and_shell_tools() {
    let toolset = make_local_toolset();
    let names: Vec<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();

    assert!(names.iter().any(|n| n == "read"), "missing read: {names:?}");
    assert!(names.iter().any(|n| n == "bash"), "missing bash: {names:?}");
    assert!(names.iter().any(|n| n == "grep"), "missing grep: {names:?}");
    assert!(names.iter().any(|n| n == "glob"), "missing glob: {names:?}");
    assert!(
        !names.is_empty(),
        "toolset should register at least some tools"
    );
}

#[test]
fn core_catalog_tool_schemas_have_descriptions() {
    let toolset = make_local_toolset();
    for schema in toolset.tools.iter() {
        assert!(
            !schema.function.description.is_empty(),
            "tool '{}' has empty description",
            schema.function.name
        );
    }
}

fn description_for(name: &str) -> String {
    let toolset = make_local_toolset();
    toolset
        .definitions
        .iter()
        .find(|d| d.name == name)
        .unwrap_or_else(|| panic!("tool '{name}' not found"))
        .description
        .clone()
}

#[test]
fn file_read_prompt_prefers_over_shell_cat() {
    let desc = description_for("read");
    assert!(
        desc.contains("cat") || desc.contains("head") || desc.contains("tail"),
        "file_read description must mention cat/head/tail preference"
    );
}

#[test]
fn file_edit_prompt_requires_read_first() {
    let desc = description_for("edit");
    assert!(
        desc.contains("read"),
        "file_edit description must require reading the file first"
    );
    assert!(
        desc.contains("unique"),
        "file_edit description must mention old_string uniqueness"
    );
}

#[test]
fn file_write_prompt_prefers_edit() {
    let desc = description_for("write");
    assert!(
        desc.contains("edit") || desc.contains("Edit"),
        "file_write description must prefer file_edit for modifications"
    );
    assert!(
        desc.contains("read"),
        "file_write description must require reading existing files first"
    );
}

#[test]
fn shell_prompt_steers_away_from_dedicated_tools() {
    let desc = description_for("bash");
    for tool in &["grep", "find", "cat", "head", "tail", "sed", "awk"] {
        assert!(
            desc.contains(tool),
            "shell description must mention avoiding '{tool}'"
        );
    }
    assert!(
        desc.contains("read") || desc.contains("Read"),
        "shell description must point to file_read"
    );
    assert!(
        desc.contains("glob") || desc.contains("Glob"),
        "shell description must point to glob"
    );
}

#[test]
fn grep_prompt_forbids_shell_grep() {
    let desc = description_for("grep");
    assert!(
        desc.contains("NEVER") || desc.contains("never"),
        "grep description must forbid shell grep"
    );
    assert!(
        desc.contains("regex") || desc.contains("ripgrep"),
        "grep description must mention regex/ripgrep support"
    );
}

#[test]
fn glob_prompt_forbids_shell_find() {
    let desc = description_for("glob");
    assert!(
        desc.contains("NEVER") || desc.contains("never"),
        "glob description must forbid shell find/ls"
    );
}

#[test]
fn web_fetch_prompt_requires_search_first() {
    let desc = description_for("web_fetch");
    assert!(
        desc.contains("web_search") || desc.contains("search"),
        "web_fetch description must reference using search first"
    );
    assert!(
        desc.contains("guess") || desc.contains("memory"),
        "web_fetch description must warn against guessing URLs"
    );
}

#[test]
fn file_read_prompt_does_not_claim_image_support() {
    let desc = description_for("read");
    assert!(
        !desc.contains("screenshot"),
        "file_read must not claim screenshot support (not implemented)"
    );
    assert!(
        !desc.contains("ALWAYS use this tool to view"),
        "file_read must not claim visual image reading capability"
    );
    assert!(
        desc.contains("text file") || desc.contains("not directories or binary"),
        "file_read must clarify it only reads text files"
    );
}

#[test]
fn grep_glob_prompt_does_not_reference_agent_tool() {
    for name in &["grep", "glob"] {
        let desc = description_for(name);
        assert!(
            !desc.contains("agent tool") && !desc.contains("Agent tool"),
            "{name} must not reference non-existent agent tool"
        );
    }
}

#[test]
fn shell_prompt_steers_to_list_dir_not_ls() {
    let desc = description_for("bash");
    assert!(
        desc.contains("list_dir"),
        "shell must steer directory listing to list_dir"
    );
    assert!(
        !desc.contains("first run ls"),
        "shell must not tell model to run ls when list_dir exists"
    );
}

#[test]
fn file_read_prompt_does_not_claim_partial_read() {
    let desc = description_for("read");
    assert!(
        !desc.contains("only read that part")
            && !desc.contains("offset")
            && !desc.contains("limit"),
        "file_read must not claim partial/ranged read support (not implemented)"
    );
}

#[test]
fn shell_prompt_does_not_claim_background_execution() {
    let desc = description_for("bash");
    assert!(
        !desc.contains("background execution") && !desc.contains("run_in_background"),
        "shell must not claim background execution support (not implemented)"
    );
}

#[test]
fn web_search_prompt_requires_sources() {
    let desc = description_for("web_search");
    assert!(
        desc.contains("Sources") || desc.contains("sources"),
        "web_search description must require citing sources"
    );
}

// ── Boundary tests ────────────────────────────────────────────────────

const EXPECTED_CORE: &[&str] = &[
    "read",
    "write",
    "edit",
    "list_dir",
    "bash",
    "glob",
    "grep",
    "web_fetch",
    "web_search",
];

#[test]
fn default_exposure_is_exactly_9_core_tools() {
    let toolset = make_local_toolset();
    let mut names: Vec<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    names.sort();
    let mut expected: Vec<&str> = EXPECTED_CORE.to_vec();
    expected.sort();
    assert_eq!(
        names, expected,
        "default tool schemas must be exactly the 9 core tools"
    );
}

#[test]
fn bindings_match_definitions() {
    let toolset = make_local_toolset();
    assert_eq!(
        toolset.bindings.len(),
        toolset.definitions.len(),
        "every definition should have a corresponding binding"
    );
    for def in toolset.definitions.iter() {
        assert!(
            toolset.bindings.contains_key(&def.name),
            "missing binding for '{}'",
            def.name
        );
    }
}

#[test]
fn filter_restricts_to_subset() {
    let filter: HashSet<String> = ["read", "bash"].iter().map(|s| s.to_string()).collect();
    let toolset = build_local_toolset(Some(filter), sink());
    let names: Vec<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"read".to_string()));
    assert!(names.contains(&"bash".to_string()));
    assert!(toolset.allowed_tool_names.is_some());
}

#[test]
fn filter_can_include_non_core_tool() {
    let filter: HashSet<String> = ["read", "list_dir"].iter().map(|s| s.to_string()).collect();
    let toolset = build_local_toolset(Some(filter), sink());
    let names: Vec<String> = toolset
        .tools
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    assert!(
        names.contains(&"list_dir".to_string()),
        "filter should expose non-core tool"
    );
}

#[test]
fn no_filter_means_no_allowed_tool_names() {
    let toolset = make_local_toolset();
    assert!(
        toolset.allowed_tool_names.is_none(),
        "no filter should mean no allowed_tool_names restriction"
    );
}

#[test]
fn tool_identity_matches_new_names() {
    use bendclaw::kernel::tools::ToolId;
    assert_eq!(ToolId::Read.as_str(), "read");
    assert_eq!(ToolId::Write.as_str(), "write");
    assert_eq!(ToolId::Edit.as_str(), "edit");
    assert_eq!(ToolId::Bash.as_str(), "bash");
    assert_eq!(ToolId::Glob.as_str(), "glob");
    assert_eq!(ToolId::Grep.as_str(), "grep");
    assert_eq!(ToolId::WebFetch.as_str(), "web_fetch");
    assert_eq!(ToolId::WebSearch.as_str(), "web_search");
}
