use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde::Serialize;

use super::session::Session;
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
