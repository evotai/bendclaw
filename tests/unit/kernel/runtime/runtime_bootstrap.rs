use std::sync::Arc;

use bendclaw::kernel::runtime::Runtime;
use bendclaw_test_harness::mocks::llm::MockLLMProvider;

fn noop_llm() -> Arc<dyn bendclaw::llm::provider::LLMProvider> {
    Arc::new(MockLLMProvider::with_text("ok"))
}

#[tokio::test]
async fn build_minimal_runtime_is_ready() {
    let runtime = Runtime::new("", "", "default", "test", "node-1", noop_llm())
        .build_minimal()
        .await
        .expect("build_minimal");
    assert_eq!(
        runtime.status(),
        bendclaw::kernel::runtime::RuntimeStatus::Ready
    );
}

#[tokio::test]
async fn build_minimal_runtime_config_reflects_builder() {
    let runtime = Runtime::new("", "", "default", "pfx", "node-2", noop_llm())
        .with_max_iterations(5)
        .with_max_context_tokens(10_000)
        .build_minimal()
        .await
        .expect("build_minimal");
    assert_eq!(runtime.config().max_iterations, 5);
    assert_eq!(runtime.config().max_context_tokens, 10_000);
}

#[tokio::test]
async fn build_minimal_runtime_sessions_start_empty() {
    let runtime = Runtime::new("", "", "default", "test", "node-3", noop_llm())
        .build_minimal()
        .await
        .expect("build_minimal");
    assert_eq!(runtime.sessions().active_count(), 0);
}

#[tokio::test]
async fn build_minimal_runtime_suspend_status_can_suspend() {
    let runtime = Runtime::new("", "", "default", "test", "node-4", noop_llm())
        .build_minimal()
        .await
        .expect("build_minimal");
    let status = runtime.suspend_status();
    assert!(status.can_suspend);
    assert_eq!(status.active_sessions, 0);
    assert_eq!(status.active_tasks, 0);
}

#[tokio::test]
async fn build_minimal_runtime_shutdown_reaches_stopped() {
    let runtime = Runtime::new("", "", "default", "test", "node-5", noop_llm())
        .build_minimal()
        .await
        .expect("build_minimal");
    runtime.shutdown().await.expect("shutdown");
    assert_eq!(
        runtime.status(),
        bendclaw::kernel::runtime::RuntimeStatus::Stopped
    );
}
