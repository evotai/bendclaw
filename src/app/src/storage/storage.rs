use async_trait::async_trait;

use crate::error::Result;
use crate::search::SessionWithText;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::VariableRecord;

#[async_trait]
pub trait Storage: Send + Sync {
    async fn save_session(&self, session: SessionMeta) -> Result<()>;
    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>>;
    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>>;
    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>>;

    async fn append_entry(&self, entry: TranscriptEntry) -> Result<()>;
    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>>;

    async fn load_variables(&self) -> Result<Vec<VariableRecord>>;
    async fn save_variables(&self, variables: Vec<VariableRecord>) -> Result<()>;
}
