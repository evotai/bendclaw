//! Tests for prompt construction helpers: truncate_layer, substitute_template,
//! and layer size constants.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use async_trait::async_trait;
use bendclaw::config::agent::AgentConfig;
use bendclaw::kernel::agent_store::AgentStore;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::planning::build_prompt;
use bendclaw::planning::substitute_template;
use bendclaw::planning::truncate_layer;
use bendclaw::planning::CloudPromptLoader;
use bendclaw::planning::PromptConfig;
use bendclaw::planning::PromptInputs;
use bendclaw::planning::PromptRequestMeta;
use bendclaw::planning::PromptResolver;
use bendclaw::planning::PromptSeed;
use bendclaw::planning::PromptVariable;
use bendclaw::planning::SkillPromptEntry;
use bendclaw::planning::MAX_ERRORS_BYTES;
use bendclaw::planning::MAX_IDENTITY_BYTES;
use bendclaw::planning::MAX_RUNTIME_BYTES;
use bendclaw::planning::MAX_SKILLS_BYTES;
use bendclaw::planning::MAX_SOUL_BYTES;
use bendclaw::planning::MAX_SYSTEM_BYTES;
use bendclaw::planning::MAX_TOOLS_BYTES;
use bendclaw::planning::MAX_VARIABLES_BYTES;
use bendclaw::skills::sync::SkillIndex;
use bendclaw::storage::AgentDatabases;
use bendclaw::tools::definition::ToolDefinition;
use bendclaw::tools::OpType;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

// ═══════════════════════════════════════════════════════════════════════════════
// truncate_layer / substitute_template
// ═══════════════════════════════════════════════════════════════════════════════

fn assert_truncate_case(name: &str, content: &str, limit: usize, expected: Expected<'_>) {
    let result = truncate_layer("test", content, limit, "test");
    match expected {
        Expected::Exact(text) => assert_eq!(result, text, "{name}"),
        Expected::Truncated { prefix, snippet } => {
            assert!(result.starts_with(prefix), "{name}");
            assert!(result.contains(snippet), "{name}");
        }
    }
}

enum Expected<'a> {
    Exact(&'a str),
    Truncated { prefix: &'a str, snippet: &'a str },
}

#[test]
fn truncate_layer_cases() {
    let cases = [
        (
            "short unchanged",
            "Hello, world!",
            1024,
            Expected::Exact("Hello, world!"),
        ),
        ("exact limit", "abcdef", 6, Expected::Exact("abcdef")),
        ("over limit", "abcdefghij", 5, Expected::Truncated {
            prefix: "abcde",
            snippet: "[... truncated at 5/10 bytes ...]",
        }),
        ("empty", "", 1024, Expected::Exact("")),
        ("utf8 boundary", "你好世界", 4, Expected::Truncated {
            prefix: "你",
            snippet: "[... truncated at 3/12 bytes ...]",
        }),
        ("emoji", "🚀🎉🔥", 5, Expected::Truncated {
            prefix: "🚀",
            snippet: "[... truncated at 4/12 bytes ...]",
        }),
        ("one byte", "abc", 1, Expected::Truncated {
            prefix: "a",
            snippet: "[... truncated at 1/3 bytes ...]",
        }),
        ("zero limit", "abc", 0, Expected::Truncated {
            prefix: "",
            snippet: "[... truncated at 0/3 bytes ...]",
        }),
        ("one byte over", "abcdefg", 6, Expected::Truncated {
            prefix: "abcdef",
            snippet: "[... truncated at 6/7 bytes ...]",
        }),
    ];

    for (name, content, limit, expected) in cases {
        assert_truncate_case(name, content, limit, expected);
    }
}

#[test]
fn substitute_template_cases() {
    let cases = [
        (
            "no placeholders",
            "Hello world",
            serde_json::json!({"key": "val"}),
            "Hello world",
        ),
        (
            "single",
            "Hello {name}!",
            serde_json::json!({"name": "Alice"}),
            "Hello Alice!",
        ),
        (
            "multiple",
            "{name} is {role}",
            serde_json::json!({"name": "Bob", "role": "admin"}),
            "Bob is admin",
        ),
        (
            "missing key preserved",
            "Hello {unknown}!",
            serde_json::json!({"name": "X"}),
            "Hello {unknown}!",
        ),
        (
            "null unchanged",
            "Hello {name}!",
            serde_json::Value::Null,
            "Hello {name}!",
        ),
        (
            "non-object unchanged",
            "Hello {name}!",
            serde_json::json!("string"),
            "Hello {name}!",
        ),
        (
            "numeric",
            "Count: {n}",
            serde_json::json!({"n": 42}),
            "Count: 42",
        ),
        (
            "boolean",
            "Active: {flag}",
            serde_json::json!({"flag": true}),
            "Active: true",
        ),
        (
            "empty string",
            "Val={v}",
            serde_json::json!({"v": ""}),
            "Val=",
        ),
        (
            "repeated",
            "{x} and {x}",
            serde_json::json!({"x": "hi"}),
            "hi and hi",
        ),
        (
            "array repr",
            "v={arr}",
            serde_json::json!({"arr": [1, 2, 3]}),
            "v=[1,2,3]",
        ),
        (
            "object repr",
            "v={obj}",
            serde_json::json!({"obj": {"a": 1}}),
            r#"v={"a":1}"#,
        ),
        (
            "null repr",
            "v={x}",
            serde_json::json!({"x": null}),
            "v=null",
        ),
    ];

    for (name, template, state, expected) in cases {
        assert_eq!(substitute_template(template, &state), expected, "{name}");
    }
}

struct NoopLLM;

// ═══════════════════════════════════════════════════════════════════════════════
// CloudPromptLoader integration
// ═══════════════════════════════════════════════════════════════════════════════

#[async_trait]
impl LLMProvider for NoopLLM {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> bendclaw::types::Result<LLMResponse> {
        Err(bendclaw::types::ErrorCode::internal("noop llm"))
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

/// Build a CloudPromptLoader backed by a FakeDatabend for DB-fetching tests.
fn make_cloud_loader(
    query: impl Fn(&str, Option<&str>) -> bendclaw::types::Result<bendclaw::storage::pool::QueryResponse>
        + Send
        + Sync
        + 'static,
    cached_config: Option<PromptConfig>,
    tools: Arc<Vec<ToolDefinition>>,
    variables: Vec<PromptVariable>,
) -> Result<(CloudPromptLoader, FakeDatabend, PathBuf)> {
    let fake = FakeDatabend::new(query);
    let pool = fake.pool();
    let workspace_root = prompt_test_workspace();
    std::fs::create_dir_all(&workspace_root)?;
    let catalog = Arc::new(SkillIndex::new(
        workspace_root.clone(),
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let llm: Arc<dyn LLMProvider> = Arc::new(NoopLLM);
    let databases = Arc::new(AgentDatabases::new(pool, "test_")?);
    let meta_pool = databases.root_pool().with_database("evotai_meta")?;
    let config = AgentConfig::default();
    let org = Arc::new(OrgServices::new(meta_pool, catalog, &config, llm.clone()));
    let agent_pool = databases.agent_pool("agent-1")?;
    let storage = Arc::new(AgentStore::new(agent_pool, llm));
    let cwd = workspace_root.clone();
    let loader = CloudPromptLoader::new(
        storage,
        org,
        tools,
        variables,
        cached_config,
        cwd,
        None,  // cluster_client
        None,  // directive
        false, // memory_enabled
        0,     // memory_recall_budget
        "agent-1".to_string(),
        "user-1".to_string(),
        "session-1".to_string(),
    );
    Ok((loader, fake, workspace_root))
}

#[test]
fn prompt_builder_build_uses_injected_layers_in_order_and_substitutes_state() -> Result<()> {
    let tools = Arc::new(vec![ToolDefinition {
        name: "shell".into(),
        description: "Run shell commands".into(),
        input_schema: serde_json::json!({"type": "object"}),
        op_type: OpType::SkillRun,
    }]);
    let variables = vec![
        PromptVariable {
            key: "PLAIN_KEY".into(),
            value: "plain-value".into(),
            secret: false,
        },
        PromptVariable {
            key: "SECRET_KEY".into(),
            value: "secret-value".into(),
            secret: true,
        },
    ];

    let prompt = build_prompt(PromptInputs {
        seed: PromptSeed {
            cached_config: Some(PromptConfig {
                system_prompt: String::new(),
                identity: "Identity for {name}".to_string(),
                soul: "Helpful soul".to_string(),
                token_limit_total: None,
                token_limit_daily: None,
            }),
            variables,
            skill_prompts: vec![SkillPromptEntry {
                display_name: "demo-skill".to_string(),
                description: "Demo skill".to_string(),
            }],
            directive_prompt: None,
        },
        tools,
        cwd: PathBuf::from("/tmp"),
        system_overlay: None,
        skill_overlay: None,
        memory_recall: None,
        cluster_info: None,
        recent_errors: Some("- `bad_tool`: failed before\n".to_string()),
        session_state: Some(serde_json::json!({"name": "Alice"})),
        channel_type: None,
        channel_chat_id: None,
        runtime_override: Some("Runtime for {name}".to_string()),
    });

    assert!(prompt.contains("Identity for Alice"));
    assert!(prompt.contains("## Soul\n\nHelpful soul"));
    assert!(prompt.contains("<skill name=\"demo-skill\">Demo skill</skill>"));
    assert!(prompt.contains("- `shell`: Run shell commands"));
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
    let variables = prompt.find("## Variables").context("missing variables")?;
    let errors = prompt.find("## Recent Errors").context("missing errors")?;
    let runtime = prompt.find("## Runtime").context("missing runtime")?;
    assert!(soul < skills && skills < tools && tools < variables);
    assert!(variables < errors && errors < runtime);

    Ok(())
}

#[tokio::test]
async fn prompt_builder_build_falls_back_to_db_layers() -> Result<()> {
    let cached_config = Some(PromptConfig {
        system_prompt: "System for {name}".to_string(),
        identity: "Identity for {name}".to_string(),
        soul: "Soul from db".to_string(),
        token_limit_total: None,
        token_limit_daily: None,
    });
    let (loader, fake, _workspace_root) = make_cloud_loader(
        |sql, _database| {
            if sql.contains("FROM sessions WHERE id = 'session-1'") {
                return Ok(paged_rows(
                    &[&[
                        "session-1",
                        "agent-1",
                        "user-1",
                        "Prompt Session",
                        "private",
                        "",
                        "",
                        "",
                        r#"{"name":"Bob"}"#,
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
        },
        cached_config,
        Arc::new(vec![]),
        vec![], /* empty variables — loader will NOT fall back to DB for variables (they are injected) */
    )?;

    let prompt = loader.resolve(&PromptRequestMeta::default()).await?;

    assert!(prompt.contains("Identity for Bob"));
    assert!(prompt.contains("## Soul\n\nSoul from db"));
    assert!(prompt.contains("System for Bob"));
    assert!(prompt.contains("## Recent Errors"));
    assert!(prompt.contains("- `shell`: command failed"));
    assert!(prompt.contains("## Runtime"));

    let calls = fake.calls();
    // With cached_config, should NOT query agent_config
    assert!(!calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM agent_config"))
    ));
    // Should query session for state
    assert!(calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM sessions WHERE id = 'session-1'"))
    ));
    // Should query recent errors from DB
    assert!(calls.iter().any(
        |call| matches!(call, FakeDatabendCall::Query { sql, .. } if sql.contains("FROM spans WHERE status = 'failed'"))
    ));
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
            + MAX_VARIABLES_BYTES
            + MAX_ERRORS_BYTES
            + MAX_RUNTIME_BYTES;
        assert!(total <= 250 * 1024);
    }
}

#[test]
fn prompt_shows_subscribed_skill_with_namespaced_key() -> Result<()> {
    let prompt = build_prompt(PromptInputs {
        seed: PromptSeed {
            cached_config: None,
            variables: vec![],
            skill_prompts: vec![
                SkillPromptEntry {
                    display_name: "demo-skill".to_string(),
                    description: "Demo skill".to_string(),
                },
                SkillPromptEntry {
                    display_name: "alice/shared-docs".to_string(),
                    description: "Alice shared docs".to_string(),
                },
            ],
            directive_prompt: None,
        },
        tools: Arc::new(vec![]),
        cwd: PathBuf::from("/tmp"),
        system_overlay: None,
        skill_overlay: None,
        memory_recall: None,
        cluster_info: None,
        recent_errors: None,
        session_state: None,
        channel_type: None,
        channel_chat_id: None,
        runtime_override: None,
    });

    // Hub skill appears with bare name
    assert!(prompt.contains("<skill name=\"demo-skill\">Demo skill</skill>"));
    // Subscribed skill appears with owner/name
    assert!(
        prompt.contains("<skill name=\"alice/shared-docs\">Alice shared docs</skill>"),
        "prompt should show subscribed skill with namespaced key, got:\n{}",
        prompt
            .lines()
            .filter(|l| l.contains("skill"))
            .collect::<Vec<_>>()
            .join("\n")
    );

    Ok(())
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

// ═══════════════════════════════════════════════════════════════════════════════
// build_prompt (pure function — no DB)
// ═══════════════════════════════════════════════════════════════════════════════

fn minimal_inputs() -> PromptInputs {
    PromptInputs {
        seed: PromptSeed::default(),
        tools: Arc::new(vec![]),
        cwd: PathBuf::from("/tmp"),
        system_overlay: None,
        skill_overlay: None,
        memory_recall: None,
        cluster_info: None,
        recent_errors: None,
        session_state: None,
        channel_type: None,
        channel_chat_id: None,
        runtime_override: None,
    }
}

#[test]
fn build_prompt_returns_nonempty_with_defaults() {
    let prompt = build_prompt(minimal_inputs());
    assert!(
        !prompt.is_empty(),
        "prompt should contain at least the default identity"
    );
}

#[test]
fn build_prompt_includes_system_overlay() {
    let mut inputs = minimal_inputs();
    inputs.system_overlay = Some("You are a test bot.".to_string());
    let prompt = build_prompt(inputs);
    assert!(prompt.contains("You are a test bot."));
}

#[test]
fn build_prompt_includes_skill_overlay() {
    let mut inputs = minimal_inputs();
    inputs.skill_overlay = Some("Custom skill instructions here.".to_string());
    let prompt = build_prompt(inputs);
    assert!(prompt.contains("Custom skill instructions here."));
}

#[test]
fn build_prompt_includes_directive() {
    let mut inputs = minimal_inputs();
    inputs.seed.directive_prompt = Some("Always respond in JSON.".to_string());
    let prompt = build_prompt(inputs);
    assert!(prompt.contains("Always respond in JSON."));
}

#[test]
fn build_prompt_includes_skill_prompts() {
    let mut inputs = minimal_inputs();
    inputs.seed.skill_prompts = vec![SkillPromptEntry {
        display_name: "test_skill".to_string(),
        description: "A test skill for unit testing.".to_string(),
    }];
    let prompt = build_prompt(inputs);
    assert!(prompt.contains("test_skill"));
    assert!(prompt.contains("A test skill for unit testing."));
}

#[test]
fn build_prompt_includes_memory_recall() {
    let mut inputs = minimal_inputs();
    inputs.memory_recall = Some("User prefers terse responses.".to_string());
    let prompt = build_prompt(inputs);
    assert!(prompt.contains("User prefers terse responses."));
}
