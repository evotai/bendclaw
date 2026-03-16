use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;

use crate::kernel::session::Session;

pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<Session>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn get(&self, id: &str) -> Option<Arc<Session>> {
        self.sessions.read().get(id).cloned()
    }

    pub fn insert(&self, session: Arc<Session>) {
        self.sessions.write().insert(session.id.clone(), session);
    }

    pub fn remove(&self, id: &str) {
        self.sessions.write().remove(id);
    }

    /// Mark all sessions for the given agent as stale and evict the idle ones.
    /// Running sessions are left in-place so in-flight work is not interrupted.
    pub fn invalidate_by_agent(&self, agent_id: &str) -> SessionInvalidation {
        let mut sessions = self.sessions.write();
        let mut marked_running = 0usize;
        let mut to_remove = Vec::new();
        for (id, session) in sessions.iter() {
            if session.agent_id_ref() != agent_id {
                continue;
            }
            session.mark_stale();
            if session.is_running() {
                marked_running += 1;
            } else {
                to_remove.push(id.clone());
            }
        }
        let evicted_idle = to_remove.len();
        for id in to_remove {
            sessions.remove(&id);
        }
        SessionInvalidation {
            evicted_idle,
            marked_running,
        }
    }

    pub async fn close_all(&self) {
        let all: Vec<Arc<Session>> = self.sessions.read().values().cloned().collect();
        for session in &all {
            session.close().await;
        }
        self.sessions.write().clear();
        tracing::info!(closed = all.len(), "all sessions closed");
    }

    pub fn stats(&self) -> SessionStats {
        let sessions = self.sessions.read();
        let mut infos = Vec::with_capacity(sessions.len());
        let mut active = 0usize;
        let mut idle = 0usize;

        for session in sessions.values() {
            if session.is_running() {
                active += 1;
            } else {
                idle += 1;
            }
            infos.push(session.info());
        }

        SessionStats {
            total: sessions.len(),
            active,
            idle,
            sessions: infos,
        }
    }

    pub fn active_count(&self) -> usize {
        self.sessions
            .read()
            .values()
            .filter(|session| session.is_running())
            .count()
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStats {
    pub total: usize,
    pub active: usize,
    pub idle: usize,
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct SessionInvalidation {
    pub evicted_idle: usize,
    pub marked_running: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub agent_id: String,
    pub user_id: String,
    pub status: String,
    pub last_active_ms: u64,
    pub current_turn: Option<TurnStats>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TurnStats {
    pub iteration: u32,
    pub duration_ms: u64,
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU32;
    use std::sync::Arc;
    use std::time::Duration;

    use async_trait::async_trait;
    use parking_lot::RwLock;
    use tokio_util::sync::CancellationToken;

    use super::SessionManager;
    use crate::base::ErrorCode;
    use crate::kernel::agent_store::AgentStore;
    use crate::kernel::runtime::agent_config::AgentConfig;
    use crate::kernel::session::session::SessionState;
    use crate::kernel::session::workspace::SandboxResolver;
    use crate::kernel::session::workspace::Workspace;
    use crate::kernel::session::Session;
    use crate::kernel::session::SessionResources;
    use crate::kernel::skills::store::SkillStore;
    use crate::kernel::tools::registry::ToolRegistry;
    use crate::llm::message::ChatMessage;
    use crate::llm::provider::LLMProvider;
    use crate::llm::provider::LLMResponse;
    use crate::llm::stream::ResponseStream;
    use crate::llm::tool::ToolSchema;
    use crate::storage::test_support::RecordingClient;

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

    fn test_session(session_id: &str, agent_id: &str) -> Arc<Session> {
        let llm: Arc<dyn LLMProvider> = Arc::new(NoopLLM);
        let workspace_dir =
            std::env::temp_dir().join(format!("bendclaw-session-manager-{}", ulid::Ulid::new()));
        let _ = std::fs::create_dir_all(&workspace_dir);
        let workspace = Arc::new(Workspace::new(
            workspace_dir.clone(),
            vec!["PATH".into(), "HOME".into()],
            std::collections::HashMap::new(),
            Duration::from_secs(5),
            1_048_576,
            Arc::new(SandboxResolver),
        ));
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
        let databases =
            Arc::new(crate::storage::AgentDatabases::new(pool.clone(), "unit_").unwrap());
        let skills = Arc::new(SkillStore::new(databases, workspace_dir, None));
        Arc::new(Session::new(
            session_id.into(),
            agent_id.into(),
            "u1".into(),
            SessionResources {
                workspace,
                tool_registry: Arc::new(ToolRegistry::new()),
                skills,
                tools: Arc::new(vec![]),
                storage: Arc::new(AgentStore::new(pool, llm.clone())),
                llm: Arc::new(RwLock::new(llm)),
                config: Arc::new(AgentConfig::default()),
                variables: vec![],
                recall: None,
                cluster_client: None,
                directive: None,
            },
        ))
    }

    #[test]
    fn invalidate_by_agent_evicts_idle_and_marks_running_sessions_stale() {
        let manager = SessionManager::new();
        let idle = test_session("idle", "a1");
        let running = test_session("running", "a1");
        let other = test_session("other", "a2");
        *running.state.lock() = SessionState::Running {
            run_id: "r1".into(),
            cancel: CancellationToken::new(),
            started_at: std::time::Instant::now(),
            iteration: Arc::new(AtomicU32::new(1)),
        };

        manager.insert(idle.clone());
        manager.insert(running.clone());
        manager.insert(other.clone());

        let result = manager.invalidate_by_agent("a1");

        assert_eq!(result.evicted_idle, 1);
        assert_eq!(result.marked_running, 1);
        assert!(manager.get("idle").is_none());
        assert!(manager.get("running").is_some());
        assert!(running.is_stale());
        assert!(running.is_running());
        assert!(!other.is_stale());
    }
}
