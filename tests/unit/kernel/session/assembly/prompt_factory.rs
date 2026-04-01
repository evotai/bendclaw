use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::assembly::local::build_local_assembly;
use bendclaw::kernel::session::assembly::local::LocalBuildOptions;
use bendclaw::kernel::session::assembly::local::LocalRuntimeDeps;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;

fn noop_llm() -> Arc<dyn bendclaw::llm::provider::LLMProvider> {
    Arc::new(MockLLMProvider::with_text("ok"))
}

fn make_deps(dir: &tempfile::TempDir) -> LocalRuntimeDeps {
    let mut config = AgentConfig::default();
    config.workspace.root_dir = dir.path().to_string_lossy().to_string();
    LocalRuntimeDeps::new(config, noop_llm())
}

#[tokio::test]
async fn build_local_prompt_resolver_is_present() {
    let dir = tempfile::tempdir().expect("tempdir");
    let deps = make_deps(&dir);
    let opts = LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    };
    let assembly = build_local_assembly(&deps, "s1", opts).expect("assembly");
    // prompt_resolver is Arc<dyn PromptResolver> — just verify it's accessible
    let _ = assembly.core.prompt_resolver;
}

#[tokio::test]
async fn build_local_assembly_labels_are_local() {
    let dir = tempfile::tempdir().expect("tempdir");
    let deps = make_deps(&dir);
    let opts = LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    };
    let assembly = build_local_assembly(&deps, "my-session", opts).expect("assembly");
    assert_eq!(assembly.labels.agent_id.as_ref(), "local");
    assert_eq!(assembly.labels.user_id.as_ref(), "cli");
    assert_eq!(assembly.labels.session_id.as_ref(), "my-session");
}
