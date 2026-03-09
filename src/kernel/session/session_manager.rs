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

    pub fn can_suspend(&self) -> bool {
        self.sessions.read().values().all(|s| s.is_idle())
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
