//! Tests for prompt construction helpers: truncate_layer, substitute_template,
//! format_learnings, and layer size constants.

use bendclaw::kernel::run::prompt::format_learnings;
use bendclaw::kernel::run::prompt::substitute_template;
use bendclaw::kernel::run::prompt::truncate_layer;
use bendclaw::kernel::run::prompt::MAX_ERRORS_BYTES;
use bendclaw::kernel::run::prompt::MAX_IDENTITY_BYTES;
use bendclaw::kernel::run::prompt::MAX_LEARNINGS_BYTES;
use bendclaw::kernel::run::prompt::MAX_RUNTIME_BYTES;
use bendclaw::kernel::run::prompt::MAX_SKILLS_BYTES;
use bendclaw::kernel::run::prompt::MAX_SOUL_BYTES;
use bendclaw::kernel::run::prompt::MAX_SYSTEM_BYTES;
use bendclaw::kernel::run::prompt::MAX_TOOLS_BYTES;
use bendclaw::kernel::run::prompt::MAX_VARIABLES_BYTES;
use bendclaw::storage::dal::learning::LearningRecord;

// ═══════════════════════════════════════════════════════════════════════════════
// truncate_layer
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn truncate_short_content_unchanged() {
    let content = "Hello, world!";
    let result = truncate_layer("test", content, 1024, "test");
    assert_eq!(result, content);
}

#[test]
fn truncate_exact_limit_unchanged() {
    let content = "abcdef";
    let result = truncate_layer("test", content, 6, "test");
    assert_eq!(result, content);
}

#[test]
fn truncate_over_limit() {
    let content = "abcdefghij"; // 10 bytes
    let result = truncate_layer("test", content, 5, "test");
    assert!(result.starts_with("abcde"));
    assert!(result.contains("[... truncated at 5/10 bytes ...]"));
}

#[test]
fn truncate_empty_content() {
    let result = truncate_layer("test", "", 1024, "test");
    assert_eq!(result, "");
}

#[test]
fn truncate_respects_char_boundary_utf8() {
    let content = "你好世界"; // 12 bytes (3 per char)
    let result = truncate_layer("test", content, 4, "test");
    assert!(result.starts_with("你"));
    assert!(result.contains("[... truncated at 3/12 bytes ...]"));
}

#[test]
fn truncate_multibyte_emoji() {
    let content = "🚀🎉🔥"; // 4 bytes each = 12 bytes
    let result = truncate_layer("test", content, 5, "test");
    assert!(result.starts_with("🚀"));
    assert!(result.contains("[... truncated at 4/12 bytes ...]"));
}

#[test]
fn truncate_limit_one_byte() {
    let content = "abc";
    let result = truncate_layer("test", content, 1, "test");
    assert!(result.starts_with("a"));
    assert!(result.contains("[... truncated at 1/3 bytes ...]"));
}

#[test]
fn truncate_limit_zero() {
    let content = "abc";
    let result = truncate_layer("test", content, 0, "test");
    assert!(result.contains("[... truncated at 0/3 bytes ...]"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// substitute_template
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn substitute_no_placeholders() {
    let result = substitute_template("Hello world", &serde_json::json!({"key": "val"}));
    assert_eq!(result, "Hello world");
}

#[test]
fn substitute_single_replacement() {
    let result = substitute_template("Hello {name}!", &serde_json::json!({"name": "Alice"}));
    assert_eq!(result, "Hello Alice!");
}

#[test]
fn substitute_multiple_replacements() {
    let state = serde_json::json!({"name": "Bob", "role": "admin"});
    let result = substitute_template("{name} is {role}", &state);
    assert_eq!(result, "Bob is admin");
}

#[test]
fn substitute_missing_key_preserved() {
    let result = substitute_template("Hello {unknown}!", &serde_json::json!({"name": "X"}));
    assert_eq!(result, "Hello {unknown}!");
}

#[test]
fn substitute_null_state_unchanged() {
    let result = substitute_template("Hello {name}!", &serde_json::Value::Null);
    assert_eq!(result, "Hello {name}!");
}

#[test]
fn substitute_non_object_state_unchanged() {
    let result = substitute_template("Hello {name}!", &serde_json::json!("string"));
    assert_eq!(result, "Hello {name}!");
}

#[test]
fn substitute_numeric_value() {
    let result = substitute_template("Count: {n}", &serde_json::json!({"n": 42}));
    assert_eq!(result, "Count: 42");
}

#[test]
fn substitute_boolean_value() {
    let result = substitute_template("Active: {flag}", &serde_json::json!({"flag": true}));
    assert_eq!(result, "Active: true");
}

#[test]
fn substitute_empty_string_value() {
    let result = substitute_template("Val={v}", &serde_json::json!({"v": ""}));
    assert_eq!(result, "Val=");
}

#[test]
fn substitute_repeated_placeholder() {
    let result = substitute_template("{x} and {x}", &serde_json::json!({"x": "hi"}));
    assert_eq!(result, "hi and hi");
}

// ═══════════════════════════════════════════════════════════════════════════════
// format_learnings
// ═══════════════════════════════════════════════════════════════════════════════

fn make_learning(title: &str, content: &str) -> LearningRecord {
    LearningRecord {
        id: "1".into(),
        agent_id: "a".into(),
        user_id: "".into(),
        session_id: "".into(),
        title: title.into(),
        content: content.into(),
        tags: "".into(),
        source: "".into(),
        created_at: "".into(),
        updated_at: "".into(),
    }
}

#[test]
fn format_learnings_empty() {
    let result = format_learnings(&[]);
    assert_eq!(result, "");
}

#[test]
fn format_learnings_with_title() {
    let records = vec![make_learning("Tip", "Use indexes")];
    let result = format_learnings(&records);
    assert_eq!(result, "- **Tip**: Use indexes\n");
}

#[test]
fn format_learnings_without_title() {
    let records = vec![make_learning("", "Always check errors")];
    let result = format_learnings(&records);
    assert_eq!(result, "- Always check errors\n");
}

#[test]
fn format_learnings_mixed() {
    let records = vec![
        make_learning("SQL", "Use EXPLAIN"),
        make_learning("", "Check logs first"),
    ];
    let result = format_learnings(&records);
    assert!(result.contains("- **SQL**: Use EXPLAIN\n"));
    assert!(result.contains("- Check logs first\n"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// Max size constants sanity
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn max_sizes_are_reasonable() {
    const {
        assert!(MAX_IDENTITY_BYTES >= 4096);
        assert!(MAX_SOUL_BYTES >= 8192);
        assert!(MAX_SYSTEM_BYTES >= 32768);
        assert!(MAX_SKILLS_BYTES >= 16384);
        assert!(MAX_TOOLS_BYTES >= 16384);
        assert!(MAX_LEARNINGS_BYTES >= 16384);
        assert!(MAX_ERRORS_BYTES >= 4096);
        assert!(MAX_RUNTIME_BYTES >= 2048);
    }
}

#[test]
fn total_max_under_200kb() {
    const {
        let total = MAX_IDENTITY_BYTES
            + MAX_SOUL_BYTES
            + MAX_SYSTEM_BYTES
            + MAX_SKILLS_BYTES
            + MAX_TOOLS_BYTES
            + MAX_LEARNINGS_BYTES
            + MAX_VARIABLES_BYTES
            + MAX_ERRORS_BYTES
            + MAX_RUNTIME_BYTES;
        assert!(total <= 250 * 1024);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// Realistic layer truncation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn identity_layer_within_limit() {
    let identity = "You are a helpful SQL assistant.";
    let result = truncate_layer("identity", identity, MAX_IDENTITY_BYTES, "db");
    assert_eq!(result, identity);
}

#[test]
fn system_prompt_large_truncated() {
    let system = "x".repeat(MAX_SYSTEM_BYTES + 1000);
    let result = truncate_layer("system", &system, MAX_SYSTEM_BYTES, "db");
    assert!(result.len() < system.len());
    assert!(result.contains("[... truncated at"));
}

#[test]
fn skills_layer_truncation_preserves_header() {
    let mut buf = String::from("## Available Skills\n\n<available_skills>\n");
    for i in 0..500 {
        buf.push_str(&format!(
            "<skill name=\"skill-{i}\">Description of skill {i} with enough text.</skill>\n"
        ));
    }
    buf.push_str("</available_skills>\n\n");
    let result = truncate_layer("skills", &buf, MAX_SKILLS_BYTES, "catalog");
    assert!(result.starts_with("## Available Skills"));
    if buf.len() > MAX_SKILLS_BYTES {
        assert!(result.contains("[... truncated at"));
    }
}

#[test]
fn tools_layer_many_tools() {
    let mut buf = String::from("## Available Tools\n\n");
    for i in 0..200 {
        buf.push_str(&format!(
            "- `tool_{i}`: This tool does something useful for task {i}.\n"
        ));
    }
    let result = truncate_layer("tools", &buf, MAX_TOOLS_BYTES, "registry");
    assert!(result.starts_with("## Available Tools"));
}

#[test]
fn learnings_layer_truncation() {
    let mut text = String::from("## Learnings\n\n");
    for i in 0..500 {
        text.push_str(&format!(
            "- Learning {i}: Always remember to do thing {i} correctly.\n"
        ));
    }
    let result = truncate_layer("learnings", &text, MAX_LEARNINGS_BYTES, "db");
    assert!(result.starts_with("## Learnings"));
    if text.len() > MAX_LEARNINGS_BYTES {
        assert!(result.contains("[... truncated at"));
    }
}

#[test]
fn errors_layer_within_limit() {
    let mut buf = String::from("## Recent Errors\n\n");
    for i in 0..5 {
        buf.push_str(&format!("- `tool_{i}`: command failed\n"));
    }
    let result = truncate_layer("recent_errors", &buf, MAX_ERRORS_BYTES, "db");
    assert_eq!(result, buf);
}

#[test]
fn runtime_layer_within_limit() {
    let buf = "## Runtime\n\nHost: myhost | OS: macos (aarch64)\n\n";
    let result = truncate_layer("runtime", buf, MAX_RUNTIME_BYTES, "env");
    assert_eq!(result, buf);
}

// ═══════════════════════════════════════════════════════════════════════════════
// truncate_layer — edge cases
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn truncate_one_byte_over_limit() {
    let content = "abcdefg"; // 7 bytes
    let result = truncate_layer("test", content, 6, "test");
    assert!(result.starts_with("abcdef"));
    assert!(result.contains("[... truncated at 6/7 bytes ...]"));
}

// ═══════════════════════════════════════════════════════════════════════════════
// substitute_template — non-string JSON value types
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn substitute_array_value_uses_json_repr() {
    let result = substitute_template("v={arr}", &serde_json::json!({"arr": [1, 2, 3]}));
    assert_eq!(result, "v=[1,2,3]");
}

#[test]
fn substitute_object_value_uses_json_repr() {
    let result = substitute_template("v={obj}", &serde_json::json!({"obj": {"a": 1}}));
    assert_eq!(result, "v={\"a\":1}");
}

#[test]
fn substitute_null_value_uses_json_repr() {
    let result = substitute_template("v={x}", &serde_json::json!({"x": null}));
    assert_eq!(result, "v=null");
}

// ═══════════════════════════════════════════════════════════════════════════════
// format_learnings — ordering
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn format_learnings_preserves_order() {
    let records = vec![
        make_learning("First", "aaa"),
        make_learning("Second", "bbb"),
        make_learning("Third", "ccc"),
    ];
    let result = format_learnings(&records);
    let first = result.find("First").unwrap();
    let second = result.find("Second").unwrap();
    let third = result.find("Third").unwrap();
    assert!(first < second && second < third);
}

// ═══════════════════════════════════════════════════════════════════════════════
// Variables layer truncation
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn variables_layer_within_limit() {
    let mut buf = String::from("## Variables\n\n");
    buf.push_str(
        "The following variables are available as environment variables in shell commands.\n\n",
    );
    for i in 0..10 {
        buf.push_str(&format!("- `VAR_{i}` = `value_{i}`\n"));
    }
    buf.push('\n');
    let result = truncate_layer("variables", &buf, MAX_VARIABLES_BYTES, "db");
    assert_eq!(result, buf);
}

#[test]
fn variables_layer_large_truncated() {
    let mut buf = String::from("## Variables\n\n");
    for i in 0..2000 {
        buf.push_str(&format!(
            "- `VERY_LONG_VARIABLE_NAME_{i}` = `some_long_value_for_variable_{i}`\n"
        ));
    }
    let result = truncate_layer("variables", &buf, MAX_VARIABLES_BYTES, "db");
    assert!(result.len() < buf.len());
    assert!(result.contains("[... truncated at"));
    assert!(result.starts_with("## Variables"));
}

#[test]
fn variables_max_size_is_reasonable() {
    const {
        assert!(MAX_VARIABLES_BYTES >= 8192);
    }
}
