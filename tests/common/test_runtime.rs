use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use bendclaw::kernel::channel::registry::ChannelRegistry;
use bendclaw::kernel::channel::supervisor::ChannelSupervisor;
use bendclaw::kernel::runtime::agent_config::AgentConfig;
use bendclaw::kernel::runtime::ActivityTracker;
use bendclaw::kernel::runtime::Runtime;
use bendclaw::kernel::runtime::RuntimeParts;
use bendclaw::kernel::runtime::RuntimeStatus;
use bendclaw::kernel::session::SessionManager;
use bendclaw::kernel::skills::store::SkillStore;
use bendclaw::llm::message::ChatMessage;
use bendclaw::llm::provider::LLMProvider;
use bendclaw::llm::provider::LLMResponse;
use bendclaw::llm::stream::ResponseStream;
use bendclaw::llm::tool::ToolSchema;
use bendclaw::storage::AgentDatabases;
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
    ) -> bendclaw::base::Result<LLMResponse> {
        Err(bendclaw::base::ErrorCode::internal("noop llm"))
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
    let skills = Arc::new(SkillStore::new(databases.clone(), workspace_root, None));
    let channels = Arc::new(ChannelRegistry::new());
    let supervisor = Arc::new(ChannelSupervisor::new(
        channels.clone(),
        Arc::new(|_, _| {}),
    ));

    Arc::new(Runtime::from_parts(RuntimeParts {
        config: AgentConfig::default(),
        databases,
        llm: RwLock::new(Arc::new(NoopLLM)),
        agent_llms: RwLock::new(HashMap::new()),
        skills,
        sessions: Arc::new(SessionManager::new()),
        channels,
        supervisor,
        status: RwLock::new(RuntimeStatus::Ready),
        sync_cancel: tokio_util::sync::CancellationToken::new(),
        sync_handle: RwLock::new(None),
        lease_handle: RwLock::new(None),
        cluster: RwLock::new(None),
        heartbeat_handle: RwLock::new(None),
        directive: RwLock::new(None),
        directive_handle: RwLock::new(None),
        activity_tracker: Arc::new(ActivityTracker::new()),
        trace_writer: bendclaw::kernel::trace::TraceWriter::noop(),
        persist_writer: bendclaw::kernel::writer::BackgroundWriter::noop("persist"),
        channel_message_writer: bendclaw::kernel::writer::BackgroundWriter::noop("channel_message"),
        outbound_queue: bendclaw::kernel::channel::delivery::outbound_queue::OutboundQueue::noop(),
        rate_limiter: std::sync::Arc::new(
            bendclaw::kernel::channel::delivery::rate_limit::OutboundRateLimiter::new(
                bendclaw::kernel::channel::delivery::rate_limit::RateLimitConfig::default(),
            ),
        ),
        health_monitor_handle: parking_lot::RwLock::new(None),
        tool_writer: bendclaw::kernel::writer::BackgroundWriter::noop("tool_write"),
        channel_session_keys: parking_lot::RwLock::new(HashMap::new()),
        turn_states: tokio::sync::Mutex::new(HashMap::new()),
    }))
}
