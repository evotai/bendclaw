//! Tests that SessionAssembly preserves the assembler-injected skill_executor
//! and that from_assembly() is a direct mapping (no silent replacement).

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::execution::skills::SkillExecutor;
use bendclaw::execution::skills::SkillOutput;
use bendclaw::kernel::runtime::session_org::LocalOrgServices;
use bendclaw::kernel::tools::definition::toolset::Toolset;
use bendclaw::kernel::trace::factory::NoopTraceFactory;
use bendclaw::sessions::backend::noop::NoopBackend;
use bendclaw::sessions::build::session_capabilities::*;

/// Mock executor that records whether it was called.
struct MockSkillExecutor {
    called: Arc<AtomicBool>,
}

#[async_trait]
impl SkillExecutor for MockSkillExecutor {
    async fn execute(
        &self,
        _skill_name: &str,
        _args: &[String],
    ) -> bendclaw::types::Result<SkillOutput> {
        self.called.store(true, Ordering::SeqCst);
        Ok(SkillOutput {
            data: None,
            error: Some("mock".to_string()),
        })
    }
}

fn build_assembly_with_mock(executor: Arc<dyn SkillExecutor>) -> SessionAssembly {
    let noop = Arc::new(NoopBackend);
    SessionAssembly {
        labels: RunLabels {
            agent_id: "a1".into(),
            user_id: "u1".into(),
            session_id: "s1".into(),
        },
        core: SessionCore {
            workspace: bendclaw_test_harness::mocks::context::test_workspace(
                std::path::PathBuf::from("/tmp/test-ws"),
            ),
            llm: Arc::new(parking_lot::RwLock::new(Arc::new(
                bendclaw_test_harness::mocks::llm::MockLLMProvider::with_text("ok"),
            )
                as Arc<dyn bendclaw::llm::provider::LLMProvider>)),
            toolset: Toolset {
                definitions: Arc::new(vec![]),
                bindings: Arc::new(std::collections::HashMap::new()),
                tools: Arc::new(vec![]),
                allowed_tool_names: None,
            },
            prompt_resolver: Arc::new(bendclaw::planning::LocalPromptResolver::new(
                bendclaw::planning::PromptSeed::default(),
                Arc::new(vec![]),
                std::path::PathBuf::from("/tmp"),
            )),
            context_provider: noop.clone(),
            run_initializer: noop,
        },
        infra: RuntimeInfra {
            store: Arc::new(bendclaw::sessions::store::json::JsonSessionStore::new(
                std::path::PathBuf::from("/tmp/test-session-store"),
            )),
            trace_factory: Arc::new(NoopTraceFactory),
            tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
            trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
            persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
        },
        agent: AgentContext {
            org: Arc::new(LocalOrgServices),
            config: Arc::new(bendclaw::config::agent::AgentConfig::default()),
            cluster_client: None,
            directive: None,
            prompt_config: None,
            prompt_variables: vec![],
            skill_executor: executor,
            memory_recaller: None,
        },
    }
}

/// Verify the assembly contract: the injected executor is callable and preserved.
#[tokio::test]
async fn assembly_skill_executor_is_callable() {
    let called = Arc::new(AtomicBool::new(false));
    let executor: Arc<dyn SkillExecutor> = Arc::new(MockSkillExecutor {
        called: called.clone(),
    });

    let assembly = build_assembly_with_mock(executor);

    // The assembly's executor is our mock — call it directly.
    assert!(!called.load(Ordering::SeqCst));
    let result = assembly.agent.skill_executor.execute("test", &[]).await;
    assert!(result.is_ok());
    assert!(
        called.load(Ordering::SeqCst),
        "mock executor should have been called"
    );
}

/// Verify from_assembly() doesn't silently replace the executor.
/// We prove this by checking the Arc identity: if from_assembly() replaced it,
/// the strong count on our mock would drop to 1 (only our local reference).
#[test]
fn from_assembly_preserves_executor_identity() {
    let called = Arc::new(AtomicBool::new(false));
    let executor: Arc<dyn SkillExecutor> = Arc::new(MockSkillExecutor {
        called: called.clone(),
    });
    let executor_weak = Arc::downgrade(&executor);

    let assembly = build_assembly_with_mock(executor);
    let _session = bendclaw::sessions::core::session::Session::from_assembly(assembly);

    // If from_assembly() replaced the executor with Noop, the weak ref would be dead.
    assert!(
        executor_weak.upgrade().is_some(),
        "session must hold the injected executor, not a replacement"
    );
}
