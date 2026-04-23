//! In-memory storage backend for side conversations.
//!
//! Stores sessions and transcript entries in memory. Nothing is written to
//! disk. This allows side conversations to reuse the full session / run_loop
//! pipeline with multi-turn context, without persisting anything.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use super::Storage;
use crate::error::Result;
use crate::search::collect_search_text;
use crate::search::SessionWithText;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::VariableRecord;

/// An in-memory storage backend — all data lives in memory and is discarded
/// on drop. Used by side conversations for multi-turn context without
/// touching disk.
pub struct MemoryStorage {
    sessions: Mutex<HashMap<String, SessionMeta>>,
    entries: Mutex<Vec<TranscriptEntry>>,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            entries: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl Storage for MemoryStorage {
    async fn save_session(&self, session: SessionMeta) -> Result<()> {
        if let Ok(mut map) = self.sessions.lock() {
            map.insert(session.session_id.clone(), session);
        }
        Ok(())
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        let result = self
            .sessions
            .lock()
            .ok()
            .and_then(|map| map.get(session_id).cloned());
        Ok(result)
    }

    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>> {
        let result = self
            .sessions
            .lock()
            .ok()
            .map(|map| {
                let mut sessions: Vec<_> = map.values().cloned().collect();
                if params.limit > 0 {
                    sessions.truncate(params.limit);
                }
                sessions
            })
            .unwrap_or_default();
        Ok(result)
    }

    async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let removed = self
            .sessions
            .lock()
            .ok()
            .and_then(|mut map| map.remove(session_id))
            .is_some();
        if removed {
            if let Ok(mut entries) = self.entries.lock() {
                entries.retain(|e| e.session_id != session_id);
            }
        }
        Ok(removed)
    }

    async fn append_entry(&self, entry: TranscriptEntry) -> Result<()> {
        if let Ok(mut entries) = self.entries.lock() {
            entries.push(entry);
        }
        Ok(())
    }

    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>> {
        let result = self
            .entries
            .lock()
            .ok()
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| e.session_id == params.session_id)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        Ok(result)
    }

    async fn load_variables(&self) -> Result<Vec<VariableRecord>> {
        Ok(vec![])
    }

    async fn save_variables(&self, _variables: Vec<VariableRecord>) -> Result<()> {
        Ok(())
    }

    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>> {
        let sessions: Vec<SessionMeta> = self
            .sessions
            .lock()
            .ok()
            .map(|map| {
                let mut s: Vec<_> = map.values().cloned().collect();
                if limit > 0 {
                    s.truncate(limit);
                }
                s
            })
            .unwrap_or_default();

        let entries: Vec<TranscriptEntry> = self
            .entries
            .lock()
            .ok()
            .map(|e| e.clone())
            .unwrap_or_default();

        let mut result = Vec::with_capacity(sessions.len());
        for session in &sessions {
            let session_entries: Vec<_> = entries
                .iter()
                .filter(|e| e.session_id == session.session_id)
                .cloned()
                .collect();
            let search_text = collect_search_text(session, &session_entries);
            result.push(SessionWithText {
                session: session.clone(),
                search_text,
            });
        }

        Ok(result)
    }
}
