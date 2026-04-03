use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::channels::routing::chat_router::ChatRouter;
use bendclaw::channels::routing::chat_router::ChatRouterConfig;
use bendclaw::channels::routing::debouncer::DebounceConfig;
use bendclaw::channels::runtime::channel_registry::ChannelRegistry;
use bendclaw::channels::runtime::supervisor::ChannelSupervisor;
use bendclaw::config::agent::AgentConfig;
use bendclaw::kernel::runtime::org::OrgServices;
use bendclaw::kernel::runtime::ActivityTracker;
use bendclaw::kernel::runtime::Runtime;
use bendclaw::kernel::runtime::RuntimeParts;
use bendclaw::kernel::runtime::RuntimeStatus;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::sessions::store::lifecycle::SessionLifecycle;
use bendclaw::sessions::SessionManager;
use bendclaw::skills::sync::SkillIndex;
use bendclaw::storage::AgentDatabases;
use bendclaw_test_harness::mocks::skill::NoopSkillStore;
use bendclaw_test_harness::mocks::skill::NoopSubscriptionStore;
use parking_lot::RwLock;

use super::fake_databend::FakeDatabend;

#[allow(dead_code)]
struct NoopLLM;

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
}

/// Build a minimal Runtime backed by a FakeDatabend for use in external tests.
#[allow(dead_code)]
pub fn test_runtime(fake: FakeDatabend) -> Arc<Runtime> {
    let pool = fake.pool();
    let databases = Arc::new(AgentDatabases::new(pool, "test_").expect("agent databases"));
    let workspace_root = std::env::temp_dir().join(format!("bendclaw-test-{}", ulid::Ulid::new()));
    let _ = std::fs::create_dir_all(&workspace_root);
    let projector = Arc::new(SkillIndex::new(
        workspace_root,
        Arc::new(NoopSkillStore),
        Arc::new(NoopSubscriptionStore),
        None,
    ));
    let config = AgentConfig::default();
    let llm: Arc<dyn LLMProvider> = Arc::new(NoopLLM);
    let meta_pool = databases
        .root_pool()
        .with_database("evotai_meta")
        .expect("meta pool");
    let org = Arc::new(OrgServices::new(meta_pool, projector.clone(), &config, llm));
    let channels = Arc::new(ChannelRegistry::new());
    let chat_router = Arc::new(ChatRouter::new(
        ChatRouterConfig::default(),
        DebounceConfig::default(),
        Arc::new(|_| Box::pin(async {})),
    ));
    let supervisor = Arc::new(ChannelSupervisor::new(
        channels.clone(),
        chat_router.clone(),
        Arc::new(bendclaw::channels::model::status::ChannelStatus::new()),
    ));
    let sessions = Arc::new(SessionManager::new());
    let persist_writer = bendclaw::kernel::writer::BackgroundWriter::noop("persist");
    let session_lifecycle = Arc::new(SessionLifecycle::new(
        databases.clone(),
        sessions.clone(),
        persist_writer.clone(),
    ));

    Arc::new(Runtime::from_parts(RuntimeParts {
        config,
        databases,
        llm: RwLock::new(Arc::new(NoopLLM)),
        agent_llms: RwLock::new(HashMap::new()),
        org,
        catalog: projector,
        sessions,
        session_lifecycle,
        channels,
        supervisor,
        chat_router,
        status: RwLock::new(RuntimeStatus::Ready),
        sync_cancel: tokio_util::sync::CancellationToken::new(),
        sync_handle: RwLock::new(None),
        lease_handle: RwLock::new(None),
        cluster: RwLock::new(None),
        heartbeat_handle: RwLock::new(None),
        directive: RwLock::new(None),
        directive_handle: RwLock::new(None),
        activity_tracker: Arc::new(ActivityTracker::new()),
        trace_writer: bendclaw::traces::TraceWriter::noop(),
        persist_writer,
        channel_message_writer: bendclaw::kernel::writer::BackgroundWriter::noop("channel_message"),
        rate_limiter: std::sync::Arc::new(
            bendclaw::channels::egress::rate_limit::OutboundRateLimiter::new(
                bendclaw::channels::egress::rate_limit::RateLimitConfig::default(),
            ),
        ),
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
    }))
}
