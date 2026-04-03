use std::sync::Arc;

use bendclaw::binding::session_builder::build_local_assembly;
use bendclaw::binding::session_builder::LocalBuildOptions;
use bendclaw::binding::session_builder::LocalRuntimeDeps;
use bendclaw::config::agent::AgentConfig;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;

fn noop_llm() -> Arc<dyn bendclaw::llm::provider::LLMProvider> {
    Arc::new(MockLLMProvider::with_text("ok"))
}

#[tokio::test]
async fn build_local_backend_constructs_without_panic() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = AgentConfig::default();
    config.workspace.root_dir = dir.path().to_string_lossy().to_string();

    let deps = LocalRuntimeDeps::new(config, noop_llm());
    let opts = LocalBuildOptions {
        cwd: Some(dir.path().to_path_buf()),
        tool_filter: None,
        llm_override: None,
    };
    let result = build_local_assembly(&deps, "test-session", opts);
    assert!(result.is_ok());
}

#[tokio::test]
async fn build_local_backend_with_llm_override() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut config = AgentConfig::default();
    config.workspace.root_dir = dir.path().to_string_lossy().to_string();

    let deps = LocalRuntimeDeps::new(config, noop_llm());
    let override_llm = Arc::new(MockLLMProvider::with_text("override"));
    let opts = LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: Some(override_llm),
    };
    let result = build_local_assembly(&deps, "session-override", opts);
    assert!(result.is_ok());
}
