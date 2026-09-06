use std::sync::Arc;

use crate::error::Result;
use crate::sessions::Session;
use crate::storage::Storage;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptItem;

/// Read-side application service. Owns visibility and replay policy, not run
/// lifecycle or storage mutation. The repository is supplied by composition.
pub struct SessionQueries {
    storage: Arc<dyn Storage>,
}

impl SessionQueries {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }

    pub async fn list(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let sessions = self
            .storage
            .list_sessions(ListSessions {
                limit: 0,
                offset: 0,
            })
            .await?;
        let mut visible = Vec::new();
        for session in sessions {
            // Limit after excluding abandoned drafts, not before.
            let is_draft = session.turns == 0
                && session.title.is_none()
                && !self
                    .storage
                    .session_has_entries(&session.session_id)
                    .await?;
            if !is_draft {
                visible.push(session);
                if limit > 0 && visible.len() == limit {
                    break;
                }
            }
        }
        Ok(visible)
    }

    pub async fn list_with_text(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::search::SessionWithText>> {
        self.storage.list_sessions_with_text(limit).await
    }

    pub async fn find(&self, id: &str) -> Result<Option<SessionMeta>> {
        self.storage.get_session(id).await
    }

    pub async fn transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        if self.find(id).await?.is_none() {
            return Ok(Vec::new());
        }
        let entries = self
            .storage
            .list_entries(ListTranscriptEntries {
                session_id: id.to_string(),
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await?;
        Ok(entries.into_iter().map(|entry| entry.item).collect())
    }

    pub async fn context_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        match Session::open(id, self.storage.clone()).await? {
            Some(session) => Ok(session.transcript().await),
            None => Ok(Vec::new()),
        }
    }

    pub async fn resume_transcript(&self, id: &str) -> Result<Vec<TranscriptItem>> {
        if self.find(id).await?.is_none() {
            return Ok(Vec::new());
        }
        Ok(super::replay::resume_transcript_items(
            self.storage.load_active_entries(id).await?,
        ))
    }
}
