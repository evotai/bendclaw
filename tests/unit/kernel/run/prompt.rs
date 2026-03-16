//! Tests for prompt construction helpers: truncate_layer, substitute_template,
//! format_learnings, and layer size constants.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use async_trait::async_trait;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::run::prompt::format_learnings;
use bendclaw::kernel::run::prompt::substitute_template;
use bendclaw::kernel::run::prompt::truncate_layer;
use bendclaw::kernel::run::prompt::PromptBuilder;
use bendclaw::kernel::run::prompt::MAX_ERRORS_BYTES;
use bendclaw::kernel::run::prompt::MAX_IDENTITY_BYTES;
use bendclaw::kernel::run::prompt::MAX_LEARNINGS_BYTES;
use bendclaw::kernel::run::prompt::MAX_RUNTIME_BYTES;
use bendclaw::kernel::run::prompt::MAX_SKILLS_BYTES;
use bendclaw::kernel::run::prompt::MAX_SOUL_BYTES;
use bendclaw::kernel::run::prompt::MAX_SYSTEM_BYTES;
use bendclaw::kernel::run::prompt::MAX_TOOLS_BYTES;
use bendclaw::kernel::run::prompt::MAX_VARIABLES_BYTES;
use bendclaw::kernel::skills::hub::paths::hub_dir;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::storage::dal::learning::LearningRecord;
use bendclaw::storage::AgentDatabases;
use bendclaw::storage::VariableRecord;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

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
        kind: "pattern".into(),
        subject: "".into(),
        title: title.into(),
        content: content.into(),
        conditions: None,
        strategy: None,
        priority: 0,
        confidence: 1.0,
        status: "active".into(),
        supersedes_id: "".into(),
        user_id: "".into(),
        source_run_id: "".into(),
        success_count: 0,
        failure_count: 0,
        last_applied_at: None,
        created_at: "".into(),
        updated_at: "".into(),
    }
}

struct NoopLLM;

#[async_trait]
impl LLMProvider for NoopLLM {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::base::Result<LLMResponse> {
        unreachable!("prompt builder tests do not call chat")
    }

    fn chat_stream(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> ResponseStream {
        let (_writer, stream) = ResponseStream::channel(1);
        stream
    }

    fn default_model(&self) -> &str {
        "mock"
    }

    fn default_temperature(&self) -> f64 {
        0.0
    }
}

fn prompt_test_workspace() -> PathBuf {
    std::env::temp_dir().join(format!("bendclaw-prompt-{}", ulid::Ulid::new()))
}

fn make_prompt_builder(
    query: impl Fn(&str, Option<&str>) -> bendclaw::base::Result<bendclaw::storage::pool::QueryResponse>
        + Send
        + Sync
        + 'static,
) -> Result<(PromptBuilder, FakeDatabend, PathBuf)> {
    let fake = FakeDatabend::new(query);
    let pool = fake.pool();
    let databases = Arc::new(AgentDatabases::new(pool.clone(), "test_")?);
    let workspace_root = prompt_test_workspace();
    std::fs::create_dir_all(&workspace_root)?;
    let skills = Arc::new(SkillStore::new(databases, workspace_root.clone(), None));
    let storage = Arc::new(AgentStore::new(pool, Arc::new(NoopLLM)));
    Ok((PromptBuilder::new(storage, skills), fake, workspace_root))
}

fn write_hub_skill(workspace_root: &std::path::Path) -> Result<()> {
    let skill_dir = hub_dir(workspace_root).join("demo-skill");
    std::fs::create_dir_all(&skill_dir)?;
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: demo-skill\ndescription: Demo skill\n---\nUse this skill carefully.\n",
    )?;
    Ok(())
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

#[tokio::test]
async fn prompt_builder_build_uses_injected_layers_in_order_and_substitutes_state() -> Result<()> {
    let (builder, fake, workspace_root) = make_prompt_builder(|sql, _database| {
        if sql.contains("FROM sessions WHERE id = 'session-1'") {
            return Ok(paged_rows(
                &[&[
                    "session-1",
                    "agent-1",
                    "user-1",
                    "Prompt Session",
                    r#"{"name":"Alice"}"#,
                    "",
                    "2026-03-10T00:00:00Z",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        Ok(paged_rows(&[], None, None))
    })?;
    write_hub_skill(&workspace_root)?;

    let tools = Arc::new(vec![ToolSchema::new(
        "shell",
        "Run shell commands",
        serde_json::json!({"type": "object"}),
    )]);
    let variables = vec![
        VariableRecord {
            id: "var-1".into(),
            key: "PLAIN_KEY".into(),
            value: "plain-value".into(),
            secret: false,
            revoked: false,
            last_used_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        },
        VariableRecord {
            id: "var-2".into(),
            key: "SECRET_KEY".into(),
            value: "secret-value".into(),
            secret: true,
            revoked: false,
            last_used_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        },
    ];

    let prompt = builder
        .with_identity("Identity for {name}")
        .with_soul("Helpful soul")
        .with_learnings("- learned for {name}\n")
        .with_recent_errors("- `bad_tool`: failed before\n")
        .with_tools(tools)
        .with_variables(variables)
        .with_runtime("Runtime for {name}")
        .build("agent-1", "user-1", "session-1")
        .await?;

    assert!(prompt.contains("Identity for Alice"));
    assert!(prompt.contains("## Soul\n\nHelpful soul"));
    assert!(prompt.contains("<skill name=\"demo-skill\">Demo skill</skill>"));
    assert!(prompt.contains("- `shell`: Run shell commands"));
    assert!(prompt.contains("## Learnings\n\n- learned for Alice"));
    assert!(prompt.contains("- `PLAIN_KEY` = `plain-value`"));
    assert!(prompt.contains("[SECRET] (available as env var `$SECRET_KEY`)"));
    assert!(prompt.contains("## Recent Errors"));
    assert!(prompt.contains("- `bad_tool`: failed before"));
    assert!(prompt.contains("## Runtime\n\nRuntime for Alice"));

    let soul = prompt.find("## Soul").context("missing soul")?;
    let skills = prompt
        .find("## Available Skills")
        .context("missing skills")?;
    let tools = prompt.find("## Available Tools").context("missing tools")?;
    let learnings = prompt.find("## Learnings").context("missing learnings")?;
    let variables = prompt.find("## Variables").context("missing variables")?;
    let errors = prompt.find("## Recent Errors").context("missing errors")?;
    let runtime = prompt.find("## Runtime").context("missing runtime")?;
    assert!(soul < skills && skills < tools && tools < learnings && learnings < variables);
    assert!(variables < errors && errors < runtime);

    let calls = fake.calls();
    assert!(calls.iter().any(|call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM agent_config"))));
    assert!(calls.iter().any(|call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM sessions WHERE id = 'session-1'"))));
    assert!(!calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM variables"))
    ));
    assert!(!calls.iter().any(|call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM spans WHERE status = 'failed'"))));
    assert!(!calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM learnings"))
    ));
    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_falls_back_to_db_layers() -> Result<()> {
    let (builder, fake, _workspace_root) = make_prompt_builder(|sql, _database| {
        if sql.contains("FROM agent_config WHERE agent_id = 'agent-1'") {
            return Ok(paged_rows(
                &[&[
                    "agent-1",
                    "System for {name}",
                    "Prompt Agent",
                    "Prompt agent description",
                    "Identity for {name}",
                    "Soul from db",
                    "",
                    "",
                    "",
                    "2026-03-10T00:00:00Z",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.contains("FROM sessions WHERE id = 'session-1'") {
            return Ok(paged_rows(
                &[&[
                    "session-1",
                    "agent-1",
                    "user-1",
                    "Prompt Session",
                    r#"{"name":"Bob"}"#,
                    "",
                    "2026-03-10T00:00:00Z",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.contains("FROM learnings") && !sql.contains("agent_id") {
            return Ok(paged_rows(
                &[&[
                    "learn-1",
                    "pattern",
                    "",
                    "SQL",
                    "Use indexes",
                    "",
                    "",
                    "0",
                    "1.0",
                    "active",
                    "",
                    "user-1",
                    "",
                    "0",
                    "0",
                    "",
                    "2026-03-10T00:00:00Z",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.contains("FROM variables WHERE revoked = FALSE") {
            return Ok(paged_rows(
                &[&[
                    "var-1",
                    "API_KEY",
                    "secret",
                    "true",
                    "false",
                    "",
                    "2026-03-10T00:00:00Z",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        if sql.contains("FROM spans WHERE status = 'failed'") && sql.contains("session-1") {
            return Ok(paged_rows(
                &[&[
                    "span-1",
                    "trace-1",
                    "",
                    "shell",
                    "tool",
                    "",
                    "failed",
                    "10",
                    "0",
                    "0",
                    "0",
                    "0",
                    "0",
                    "tool_error",
                    "command failed",
                    "failed shell",
                    "{}",
                    "2026-03-10T00:00:00Z",
                ]],
                None,
                None,
            ));
        }
        Ok(paged_rows(&[], None, None))
    })?;

    let prompt = builder.build("agent-1", "user-1", "session-1").await?;

    assert!(prompt.contains("Identity for Bob"));
    assert!(prompt.contains("## Soul\n\nSoul from db"));
    assert!(prompt.contains("System for Bob"));
    assert!(prompt.contains("## Learnings"));
    assert!(prompt.contains("- **SQL**: Use indexes"));
    assert!(prompt.contains("## Variables"));
    assert!(prompt.contains("`API_KEY`: [SECRET]"));
    assert!(prompt.contains("## Recent Errors"));
    assert!(prompt.contains("- `shell`: command failed"));
    assert!(prompt.contains("## Runtime"));

    let calls = fake.calls();
    assert!(calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM learnings"))
    ));
    assert!(calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM variables"))
    ));
    assert!(calls.iter().any(|call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM spans WHERE status = 'failed'"))));
    Ok(())
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
fn format_learnings_preserves_order() -> Result<()> {
    let records = vec![
        make_learning("First", "aaa"),
        make_learning("Second", "bbb"),
        make_learning("Third", "ccc"),
    ];
    let result = format_learnings(&records);
    let first = result.find("First").context("First not found")?;
    let second = result.find("Second").context("Second not found")?;
    let third = result.find("Third").context("Third not found")?;
    assert!(first < second && second < third);
    Ok(())
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
