//! Tests for tools/catalog — verifies each catalog layer registers expected tools.

use std::sync::Arc;

use bendclaw::kernel::tools::registry::ToolRegistry;
use bendclaw::kernel::tools::services::NoopSecretUsageSink;

#[test]
fn core_catalog_registers_file_and_shell_tools() {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    bendclaw::kernel::tools::catalog::register_core(&mut registry, sink);

    let schemas = registry.tool_schemas();
    let names: Vec<String> = schemas.iter().map(|t| t.function.name.clone()).collect();

    // Core tools should include file, search, shell, web
    assert!(
        names.iter().any(|n| n.contains("file_read")),
        "missing file_read: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("shell")),
        "missing shell: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("grep")),
        "missing grep: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("glob")),
        "missing glob: {names:?}"
    );
    assert!(
        !names.is_empty(),
        "core catalog should register at least some tools"
    );
}

#[test]
fn core_catalog_tool_schemas_have_descriptions() {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    bendclaw::kernel::tools::catalog::register_core(&mut registry, sink);

    for schema in registry.tool_schemas() {
        assert!(
            !schema.function.description.is_empty(),
            "tool '{}' has empty description",
            schema.function.name
        );
    }
}

// ── Prompt contract tests ──────────────────────────────────────────────────
// These pin key behavioral semantics so prompt drift is caught at CI time.

fn description_for(name: &str) -> String {
    let mut registry = ToolRegistry::new();
    let sink: Arc<dyn bendclaw::kernel::tools::services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);
    bendclaw::kernel::tools::catalog::register_core(&mut registry, sink);
    registry
        .tool_schemas()
        .into_iter()
        .find(|s| s.function.name == name)
        .unwrap_or_else(|| panic!("tool '{name}' not found"))
        .function
        .description
}

#[test]
fn file_read_prompt_prefers_over_shell_cat() {
    let desc = description_for("file_read");
    assert!(
        desc.contains("cat") || desc.contains("head") || desc.contains("tail"),
        "file_read description must mention cat/head/tail preference"
    );
}

#[test]
fn file_edit_prompt_requires_read_first() {
    let desc = description_for("file_edit");
    assert!(
        desc.contains("file_read") || desc.contains("read"),
        "file_edit description must require reading the file first"
    );
    assert!(
        desc.contains("unique"),
        "file_edit description must mention old_string uniqueness"
    );
}

#[test]
fn file_write_prompt_prefers_edit() {
    let desc = description_for("file_write");
    assert!(
        desc.contains("file_edit") || desc.contains("Edit"),
        "file_write description must prefer file_edit for modifications"
    );
    assert!(
        desc.contains("read") || desc.contains("file_read"),
        "file_write description must require reading existing files first"
    );
}

#[test]
fn shell_prompt_steers_away_from_dedicated_tools() {
    let desc = description_for("shell");
    for tool in &["grep", "find", "cat", "head", "tail", "sed", "awk"] {
        assert!(
            desc.contains(tool),
            "shell description must mention avoiding '{tool}'"
        );
    }
    assert!(
        desc.contains("file_read") || desc.contains("Read"),
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
    let desc = description_for("file_read");
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
    let desc = description_for("shell");
    assert!(
        desc.contains("list_dir"),
        "shell must steer directory listing to list_dir"
    );
    // Should not simultaneously tell model to use ls
    assert!(
        !desc.contains("first run ls"),
        "shell must not tell model to run ls when list_dir exists"
    );
}

#[test]
fn file_read_prompt_does_not_claim_partial_read() {
    let desc = description_for("file_read");
    assert!(
        !desc.contains("only read that part")
            && !desc.contains("offset")
            && !desc.contains("limit"),
        "file_read must not claim partial/ranged read support (not implemented)"
    );
}

#[test]
fn shell_prompt_does_not_claim_background_execution() {
    let desc = description_for("shell");
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
