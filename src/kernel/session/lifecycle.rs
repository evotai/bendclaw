use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;

use crate::base::new_session_id;
use crate::base::ErrorCode;
use crate::base::Result;
use crate::kernel::run::persist_op::PersistOp;
use crate::kernel::run::persist_op::PersistWriter;
use crate::kernel::session::SessionManager;
use crate::observability::log::slog;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionRepo;
use crate::storage::dal::session::repo::SessionWrite;
use crate::storage::time;
use crate::storage::AgentDatabases;

#[derive(Debug, Clone)]
struct SessionDraft {
    session_id: String,
    agent_id: String,
    user_id: String,
    title: String,
    base_key: String,
    session_state: serde_json::Value,
    meta: serde_json::Value,
}

#[derive(Clone)]
pub struct SessionLifecycle {
    databases: Arc<AgentDatabases>,
    sessions: Arc<SessionManager>,
    writer: PersistWriter,
    active_by_base_key: Arc<RwLock<HashMap<String, SessionRecord>>>,
    known_sessions: Arc<RwLock<HashMap<String, SessionRecord>>>,
}

impl SessionLifecycle {
    pub fn new(
        databases: Arc<AgentDatabases>,
        sessions: Arc<SessionManager>,
        writer: PersistWriter,
    ) -> Self {
        Self {
            databases,
            sessions,
            writer,
            active_by_base_key: Arc::new(RwLock::new(HashMap::new())),
            known_sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn resolve_active(
        &self,
        agent_id: &str,
        user_id: &str,
        base_key: &str,
    ) -> Result<SessionRecord> {
        if let Some(record) = self.lookup_active(agent_id, base_key) {
            self.ensure_owner(&record, agent_id, user_id)?;
            return Ok(record);
        }

        let repo = self.repo(agent_id)?;
        if let Some(record) = repo.load_active_by_base_key(base_key).await? {
            self.ensure_owner(&record, agent_id, user_id)?;
            self.remember_session(record.clone());
            slog!(info, "session", "resolved",
                base_key,
                session_id = %record.id,
                source = "db",
            );
            return Ok(record);
        }

        let record = self.make_record(SessionDraft {
            session_id: new_session_id(),
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            title: String::new(),
            base_key: base_key.to_string(),
            session_state: serde_json::Value::Null,
            meta: serde_json::Value::Null,
        });
        self.remember_session(record.clone());
        self.stage_upsert(&repo, &record);
        slog!(info, "session", "resolved",
            base_key,
            session_id = %record.id,
            source = "created",
        );
        Ok(record)
    }

    pub async fn start_new(
        &self,
        agent_id: &str,
        user_id: &str,
        base_key: &str,
        reset_reason: &str,
    ) -> Result<SessionRecord> {
        let repo = self.repo(agent_id)?;
        let previous = match self.lookup_active(agent_id, base_key) {
            Some(record) => Some(record),
            None => repo.load_active_by_base_key(base_key).await?,
        };
        if let Some(ref record) = previous {
            self.ensure_owner(record, agent_id, user_id)?;
        }

        let record = self.make_record(SessionDraft {
            session_id: new_session_id(),
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            title: String::new(),
            base_key: base_key.to_string(),
            session_state: serde_json::Value::Null,
            meta: serde_json::Value::Null,
        });

        if let Some(mut previous) = previous {
            previous.replaced_by_session_id = record.id.clone();
            previous.reset_reason = reset_reason.to_string();
            previous.updated_at = record.updated_at.clone();
            self.remember_replaced(previous.clone());
            self.remember_session(record.clone());
            self.stage_upsert(&repo, &record);
            self.writer.send(PersistOp::SessionMarkReplaced {
                repo,
                session_id: previous.id.clone(),
                replaced_by_session_id: record.id.clone(),
                reset_reason: reset_reason.to_string(),
            });
            self.close_live_session(&previous.id).await;
            slog!(info, "session", "replaced",
                base_key,
                previous_session_id = %previous.id,
                session_id = %record.id,
                reset_reason,
            );
        } else {
            self.remember_session(record.clone());
            self.stage_upsert(&repo, &record);
            slog!(info, "session", "started",
                base_key,
                session_id = %record.id,
                reset_reason,
            );
        }

        Ok(record)
    }

    pub async fn create_direct(
        &self,
        agent_id: &str,
        user_id: &str,
        title: Option<&str>,
        session_state: Option<&serde_json::Value>,
        meta: Option<&serde_json::Value>,
    ) -> Result<SessionRecord> {
        let repo = self.repo(agent_id)?;
        let record = self.make_record(SessionDraft {
            session_id: new_session_id(),
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            title: title.unwrap_or_default().to_string(),
            base_key: String::new(),
            session_state: session_state.cloned().unwrap_or(serde_json::Value::Null),
            meta: meta.cloned().unwrap_or(serde_json::Value::Null),
        });
        self.remember_session(record.clone());
        self.stage_upsert(&repo, &record);
        slog!(info, "session", "created",
            session_id = %record.id,
            user_id,
            base_key = "",
        );
        Ok(record)
    }

    pub async fn ensure_direct(
        &self,
        agent_id: &str,
        user_id: &str,
        session_id: &str,
    ) -> Result<SessionRecord> {
        if let Some(record) = self.lookup_session(session_id) {
            self.ensure_owner(&record, agent_id, user_id)?;
            return Ok(record);
        }

        let repo = self.repo(agent_id)?;
        if let Some(record) = repo.load(session_id).await? {
            self.ensure_owner(&record, agent_id, user_id)?;
            self.remember_session(record.clone());
            return Ok(record);
        }

        let record = self.make_record(SessionDraft {
            session_id: session_id.to_string(),
            agent_id: agent_id.to_string(),
            user_id: user_id.to_string(),
            title: String::new(),
            base_key: String::new(),
            session_state: serde_json::Value::Null,
            meta: serde_json::Value::Null,
        });
        self.remember_session(record.clone());
        self.stage_upsert(&repo, &record);
        Ok(record)
    }

    pub async fn load_session(
        &self,
        agent_id: &str,
        session_id: &str,
    ) -> Result<Option<SessionRecord>> {
        if let Some(record) = self.lookup_session(session_id) {
            return Ok(Some(record));
        }
        let repo = self.repo(agent_id)?;
        let record = repo.load(session_id).await?;
        if let Some(ref record) = record {
            self.remember_session(record.clone());
        }
        Ok(record)
    }

    pub async fn update_session(
        &self,
        agent_id: &str,
        session_id: &str,
        user_id: &str,
        title: Option<&str>,
        session_state: Option<&serde_json::Value>,
        meta: Option<&serde_json::Value>,
    ) -> Result<SessionRecord> {
        let current = self
            .load_session(agent_id, session_id)
            .await?
            .ok_or_else(|| ErrorCode::internal(format!("session '{session_id}' not found")))?;
        self.ensure_owner(&current, agent_id, user_id)?;

        let mut updated = current.clone();
        if let Some(title) = title {
            updated.title = title.to_string();
        }
        if let Some(state) = session_state {
            updated.session_state = state.clone();
        }
        if let Some(meta) = meta {
            updated.meta = meta.clone();
        }
        updated.updated_at = time::now().to_rfc3339();

        let repo = self.repo(agent_id)?;
        self.remember_session(updated.clone());
        self.stage_upsert(&repo, &updated);
        Ok(updated)
    }

    pub async fn delete_session(&self, agent_id: &str, session_id: &str) -> Result<()> {
        let record = match self.lookup_session(session_id) {
            Some(record) => Some(record),
            None => {
                let repo = self.repo(agent_id)?;
                repo.load(session_id).await?
            }
        };
        self.evict_session(session_id, record.as_ref());
        self.close_live_session(session_id).await;
        self.writer.send(PersistOp::SessionDelete {
            repo: self.repo(agent_id)?,
            session_id: session_id.to_string(),
        });
        Ok(())
    }

    fn repo(&self, agent_id: &str) -> Result<SessionRepo> {
        let pool = self.databases.agent_pool(agent_id)?;
        Ok(SessionRepo::new(pool))
    }

    fn make_record(&self, draft: SessionDraft) -> SessionRecord {
        let now = time::now().to_rfc3339();
        SessionRecord {
            id: draft.session_id,
            agent_id: draft.agent_id,
            user_id: draft.user_id,
            title: draft.title,
            scope: "private".to_string(),
            base_key: draft.base_key,
            replaced_by_session_id: String::new(),
            reset_reason: String::new(),
            session_state: draft.session_state,
            meta: draft.meta,
            created_at: now.clone(),
            updated_at: now,
        }
    }

    fn stage_upsert(&self, repo: &SessionRepo, record: &SessionRecord) {
        self.writer.send(PersistOp::SessionUpsert {
            repo: repo.clone(),
            record: SessionWrite::from_record(record),
        });
    }

    fn ensure_owner(&self, record: &SessionRecord, agent_id: &str, user_id: &str) -> Result<()> {
        if record.agent_id == agent_id && record.user_id == user_id {
            return Ok(());
        }
        Err(ErrorCode::denied(format!(
            "session '{}' belongs to a different agent/user",
            record.id
        )))
    }

    fn remember_session(&self, record: SessionRecord) {
        self.known_sessions
            .write()
            .insert(record.id.clone(), record.clone());
        if !record.base_key.is_empty() && record.replaced_by_session_id.is_empty() {
            self.active_by_base_key
                .write()
                .insert(active_index(&record.agent_id, &record.base_key), record);
        }
    }

    fn remember_replaced(&self, record: SessionRecord) {
        self.known_sessions
            .write()
            .insert(record.id.clone(), record.clone());
        if !record.base_key.is_empty() {
            let key = active_index(&record.agent_id, &record.base_key);
            let mut active = self.active_by_base_key.write();
            let should_remove = active
                .get(&key)
                .map(|current| current.id == record.id)
                .unwrap_or(false);
            if should_remove {
                active.remove(&key);
            }
        }
    }

    fn evict_session(&self, session_id: &str, record: Option<&SessionRecord>) {
        self.known_sessions.write().remove(session_id);
        if let Some(record) = record {
            if !record.base_key.is_empty() {
                let key = active_index(&record.agent_id, &record.base_key);
                let mut active = self.active_by_base_key.write();
                let should_remove = active
                    .get(&key)
                    .map(|current| current.id == session_id)
                    .unwrap_or(false);
                if should_remove {
                    active.remove(&key);
                }
            }
        }
    }

    fn lookup_active(&self, agent_id: &str, base_key: &str) -> Option<SessionRecord> {
        self.active_by_base_key
            .read()
            .get(&active_index(agent_id, base_key))
            .cloned()
    }

    fn lookup_session(&self, session_id: &str) -> Option<SessionRecord> {
        self.known_sessions.read().get(session_id).cloned()
    }

    async fn close_live_session(&self, session_id: &str) {
        if let Some(session) = self.sessions.get(session_id) {
            session.close().await;
            self.sessions.remove(session_id);
        }
    }
}

fn active_index(agent_id: &str, base_key: &str) -> String {
    format!("{agent_id}:{base_key}")
}
