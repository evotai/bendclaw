use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use parking_lot::RwLock;

use crate::base::ErrorCode;
use crate::kernel::channel::registry::ChannelRegistry;
use crate::kernel::channel::supervisor::ChannelSupervisor;
use crate::kernel::runtime::agent_config::AgentConfig;
use crate::kernel::runtime::runtime::RuntimeParts;
use crate::kernel::runtime::ActivityTracker;
use crate::kernel::runtime::Runtime;
use crate::kernel::runtime::RuntimeStatus;
use crate::kernel::session::SessionManager;
use crate::kernel::skills::store::SkillStore;
use crate::llm::message::ChatMessage;
use crate::llm::provider::LLMProvider;
use crate::llm::provider::LLMResponse;
use crate::llm::stream::ResponseStream;
use crate::llm::tool::ToolSchema;
use crate::service::state::AppState;
use crate::storage::test_support::RecordingClient;
use crate::storage::AgentDatabases;

struct NoopLLM;

#[async_trait]
impl LLMProvider for NoopLLM {
    async fn chat(
        &self,
        _model: &str,
        _messages: &[ChatMessage],
        _tools: &[ToolSchema],
        _temperature: f64,
    ) -> crate::base::Result<LLMResponse> {
        Err(ErrorCode::internal("noop llm"))
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

pub(crate) fn test_runtime(test_name: &str) -> Arc<Runtime> {
    let client = RecordingClient::new(|_sql, _database| {
        Ok(crate::storage::pool::QueryResponse {
            id: String::new(),
            state: "Succeeded".to_string(),
            error: None,
            data: Vec::new(),
            next_uri: None,
            final_uri: None,
            schema: Vec::new(),
        })
    });
    let pool = client.pool();
    let databases = Arc::new(AgentDatabases::new(pool, "test_").expect("agent databases"));
    let workspace_root =
        std::env::temp_dir().join(format!("bendclaw-{test_name}-{}", ulid::Ulid::new()));
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
        cluster: None,
        heartbeat_handle: RwLock::new(None),
        directive: None,
        directive_handle: RwLock::new(None),
        activity_tracker: Arc::new(ActivityTracker::new()),
    }))
}

pub(crate) fn test_app_state(auth_key: &str) -> AppState {
    AppState {
        runtime: test_runtime("service"),
        auth_key: auth_key.to_string(),
    }
}
