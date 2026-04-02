use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::skills::sync::SkillIndex;
use bendclaw::kernel::tools::definition::toolset::Toolset;
use bendclaw::kernel::tools::tool_services::NoopSecretUsageSink;
use bendclaw::kernel::tools::ToolId;

fn make_toolset() -> Toolset {
    let pool = bendclaw::storage::Pool::new("https://api.databend.com/v1", "test-token", "default")
        .expect("pool: static URL is always valid");
    let dir = std::env::temp_dir().join(format!("bendclaw-reg-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    let skill_store = Arc::new(
        bendclaw::kernel::skills::store::DatabendSharedSkillStore::new(
            pool.with_database("evotai_meta")
                .expect("meta pool for projector"),
        ),
    );
    let projector = Arc::new(SkillIndex::new(
        dir,
        skill_store,
        Arc::new(bendclaw_test_harness::mocks::skill::NoopSubscriptionStore),
        None,
    ));
    let config = AgentConfig::default();
    let llm: Arc<dyn bendclaw::llm::provider::LLMProvider> =
        Arc::new(bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"));
    let meta_pool = pool.with_database("evotai_meta").expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector, &config, llm));
    let channels =
        Arc::new(bendclaw::kernel::channels::runtime::channel_registry::ChannelRegistry::new());
    let secret_sink: Arc<dyn bendclaw::kernel::tools::tool_services::SecretUsageSink> =
        Arc::new(NoopSecretUsageSink);

    bendclaw::kernel::tools::selection::build_cloud_toolset(
        bendclaw::kernel::tools::selection::CloudToolsetDeps {
            org,
            databend_pool: pool,
            channels,
            node_id: "test_instance".to_string(),
            cluster: None,
            memory: None,
            secret_sink,
            user_id: "test-user".to_string(),
        },
        None,
    )
}

#[test]
fn session_tools_registers_all_builtins() {
    let toolset = make_toolset();
    let expected = ToolId::ALL;
    for id in expected {
        assert!(
            toolset.bindings.contains_key(id.as_str()),
            "missing tool binding: {}",
            id.as_str()
        );
    }
}

#[test]
fn definitions_list_returns_all_names() {
    let toolset = make_toolset();
    assert!(!toolset.definitions.is_empty());
}

#[test]
fn bindings_get_unknown_returns_none() {
    let toolset = make_toolset();
    assert!(!toolset.bindings.contains_key("nonexistent_tool"));
}

#[test]
fn tool_schemas_count() {
    let toolset = make_toolset();
    assert!(!toolset.tools.is_empty());
}

#[test]
fn definitions_contain_expected_tools() {
    let toolset = make_toolset();
    let names: Vec<&str> = toolset
        .definitions
        .iter()
        .map(|d| d.name.as_str())
        .collect();
    assert!(names.contains(&"bash"), "missing bash in definitions");
    assert!(names.contains(&"read"), "missing read in definitions");
}

#[test]
fn empty_toolset() {
    let toolset = Toolset {
        definitions: Arc::new(vec![]),
        bindings: Arc::new(std::collections::HashMap::new()),
        tools: Arc::new(vec![]),
        allowed_tool_names: None,
    };
    assert!(toolset.definitions.is_empty());
    assert!(toolset.bindings.is_empty());
    assert!(toolset.tools.is_empty());
}
