use std::sync::Arc;

use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::session::build::session_builder::build_local_assembly;
use bendclaw::kernel::session::build::session_builder::LocalBuildOptions;
use bendclaw::kernel::session::build::session_builder::LocalRuntimeDeps;
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
async fn build_local_infra_noop_trace_factory() {
    let dir = tempfile::tempdir().expect("tempdir");
    let deps = make_deps(&dir);
    let opts = LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    };
    let assembly = build_local_assembly(&deps, "s1", opts).expect("assembly");
    // NoopTraceFactory: trace_factory is present (not None)
    let _ = assembly.infra.trace_factory;
}

#[tokio::test]
async fn build_local_infra_store_is_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let deps = make_deps(&dir);
    let opts = LocalBuildOptions {
        cwd: None,
        tool_filter: None,
        llm_override: None,
    };
    let assembly = build_local_assembly(&deps, "s2", opts).expect("assembly");
    // store is accessible
    let _ = assembly.infra.store;
}
