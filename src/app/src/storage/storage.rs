use async_trait::async_trait;

use crate::agent::ListSessions;
use crate::agent::ListTranscriptEntries;
use crate::agent::SessionMeta;
use crate::agent::TranscriptEntry;
use crate::error::Result;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_session(&self, session: SessionMeta) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>>;
    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>>;

    async fn append_entry(&self, entry: TranscriptEntry) -> Result<()>;
    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>>;
}
