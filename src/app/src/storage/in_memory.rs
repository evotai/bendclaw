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
use crate::search::collect_user_prompts;
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
    favorites: Mutex<Vec<String>>,
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
            favorites: Mutex::new(Vec::new()),
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
                // Newest first, matching the fs backend's ordering.
                let mut sessions: Vec<_> = map.values().cloned().collect();
                sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
                sessions
                    .into_iter()
                    .skip(params.offset)
                    .take(if params.limit == 0 {
                        usize::MAX
                    } else {
                        params.limit
                    })
                    .collect()
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

    async fn append_entries(&self, batch: Vec<TranscriptEntry>) -> Result<()> {
        let expected_seq = batch
            .first()
            .map(|entry| entry.seq.saturating_sub(1))
            .unwrap_or(0);
        if !self.compare_and_append_entries(expected_seq, batch).await? {
            return Err(crate::error::EvotError::Store(format!(
                "transcript sequence conflict: expected seq {expected_seq}"
            )));
        }
        Ok(())
    }

    async fn compare_and_append_entries(
        &self,
        expected_seq: u64,
        batch: Vec<TranscriptEntry>,
    ) -> Result<bool> {
        let Some(first) = batch.first() else {
            return Ok(true);
        };
        if batch
            .iter()
            .any(|entry| entry.session_id != first.session_id)
            || batch
                .windows(2)
                .any(|pair| pair[1].seq != pair[0].seq.saturating_add(1))
        {
            return Err(crate::error::EvotError::Store(
                "invalid transcript batch".to_string(),
            ));
        }
        let mut entries = self
            .entries
            .lock()
            .map_err(|_| crate::error::EvotError::Store("entries lock poisoned".to_string()))?;
        let persisted_seq = entries
            .iter()
            .filter(|entry| entry.session_id == first.session_id)
            .map(|entry| entry.seq)
            .max()
            .unwrap_or(0);
        if persisted_seq != expected_seq || first.seq != expected_seq.saturating_add(1) {
            return Ok(false);
        }
        entries.extend(batch);
        Ok(true)
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

    async fn upsert_variable(&self, _record: VariableRecord) -> Result<Vec<VariableRecord>> {
        Ok(vec![])
    }

    async fn remove_variable(&self, _key: String) -> Result<(bool, Vec<VariableRecord>)> {
        Ok((false, vec![]))
    }

    async fn load_favorites(&self) -> Result<Vec<String>> {
        Ok(self
            .favorites
            .lock()
            .ok()
            .map(|f| f.clone())
            .unwrap_or_default())
    }

    async fn save_favorites(&self, ids: Vec<String>) -> Result<()> {
        if let Ok(mut f) = self.favorites.lock() {
            *f = ids;
        }
        Ok(())
    }

    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>> {
        let sessions = self
            .list_sessions(ListSessions { limit, offset: 0 })
            .await?;

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
                user_prompts: collect_user_prompts(&session_entries),
            });
        }

        Ok(result)
    }
}
