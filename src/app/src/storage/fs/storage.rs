use std::path::Path;
use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;

use crate::error::EvotError;
use crate::error::Result;
use crate::search::collect_search_text;
use crate::search::SessionWithText;
use crate::storage::Storage;
use crate::types::ListSessions;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::VariableRecord;
use crate::types::VariablesDocument;

pub struct FsStorage {
    root_dir: PathBuf,
}

impl FsStorage {
    pub fn new(root_dir: PathBuf) -> Self {
        Self { root_dir }
    }

    fn sessions_dir(&self) -> PathBuf {
        self.root_dir.join("sessions")
    }

    fn session_dir(&self, session_id: &str) -> PathBuf {
        self.sessions_dir().join(session_id)
    }

    fn session_meta_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("session.json")
    }

    fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("transcript.jsonl")
    }

    fn variables_path(&self) -> PathBuf {
        self.root_dir.join("variables.json")
    }

    async fn write_json<T: serde::Serialize>(&self, path: PathBuf, value: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let json = serde_json::to_string_pretty(value)?;
        fs::write(path, json).await?;
        Ok(())
    }

    async fn read_json<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<Option<T>> {
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(serde_json::from_str(&content)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(EvotError::Io(e)),
        }
    }

    async fn append_jsonl<T: serde::Serialize>(&self, path: PathBuf, value: &T) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut line = serde_json::to_string(value)?;
        line.push('\n');
        use tokio::io::AsyncWriteExt;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    async fn read_jsonl<T: serde::de::DeserializeOwned>(&self, path: &Path) -> Result<Vec<T>> {
        match fs::read_to_string(path).await {
            Ok(content) => {
                let mut values = Vec::new();
                for line in content.lines() {
                    if !line.trim().is_empty() {
                        values.push(serde_json::from_str(line)?);
                    }
                }
                Ok(values)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(EvotError::Io(e)),
        }
    }
}

#[async_trait]
impl Storage for FsStorage {
    async fn save_session(&self, session: SessionMeta) -> Result<()> {
        self.write_json(self.session_meta_path(&session.session_id), &session)
            .await
    }

    async fn get_session(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        self.read_json(&self.session_meta_path(session_id)).await
    }

    async fn list_sessions(&self, params: ListSessions) -> Result<Vec<SessionMeta>> {
        let mut entries = match fs::read_dir(self.sessions_dir()).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(EvotError::Io(e)),
        };

        let mut sessions = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            // Skip non-directory entries (e.g. .DS_Store)
            match entry.file_type().await {
                Ok(ft) if ft.is_dir() => {}
                Ok(_) => continue,
                Err(e) => {
                    tracing::warn!(path = ?entry.path(), "skipping session entry: {e}");
                    continue;
                }
            }
            let path = entry.path().join("session.json");
            match self.read_json::<SessionMeta>(&path).await {
                Ok(Some(session)) => sessions.push(session),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!(path = ?path, "skipping malformed session.json: {e}");
                }
            }
        }

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        if params.limit > 0 {
            sessions.truncate(params.limit);
        }
        Ok(sessions)
    }

    async fn delete_session(&self, session_id: &str) -> Result<bool> {
        let dir = self.session_dir(session_id);
        match fs::remove_dir_all(&dir).await {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(EvotError::Io(e)),
        }
    }

    async fn append_entry(&self, entry: TranscriptEntry) -> Result<()> {
        self.append_jsonl(self.transcript_path(&entry.session_id), &entry)
            .await
    }

    async fn list_entries(&self, params: ListTranscriptEntries) -> Result<Vec<TranscriptEntry>> {
        let mut entries = self
            .read_jsonl::<TranscriptEntry>(&self.transcript_path(&params.session_id))
            .await?;

        if let Some(run_id) = &params.run_id {
            entries.retain(|entry| entry.run_id.as_ref() == Some(run_id));
        }
        if let Some(after_seq) = params.after_seq {
            entries.retain(|entry| entry.seq > after_seq);
        }
        if let Some(limit) = params.limit {
            entries.truncate(limit);
        }

        Ok(entries)
    }

    async fn load_variables(&self) -> Result<Vec<VariableRecord>> {
        match self
            .read_json::<VariablesDocument>(&self.variables_path())
            .await?
        {
            Some(doc) => Ok(doc.variables),
            None => Ok(Vec::new()),
        }
    }

    async fn save_variables(&self, variables: Vec<VariableRecord>) -> Result<()> {
        let doc = VariablesDocument {
            version: 1,
            variables,
        };
        self.write_json(self.variables_path(), &doc).await
    }

    async fn list_sessions_with_text(&self, limit: usize) -> Result<Vec<SessionWithText>> {
        let sessions = self.list_sessions(ListSessions { limit }).await?;
        let mut result = Vec::with_capacity(sessions.len());

        for session in &sessions {
            let entries: Vec<TranscriptEntry> = match self
                .read_jsonl(&self.transcript_path(&session.session_id))
                .await
            {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(
                        session_id = %session.session_id,
                        "skipping transcript: {e}"
                    );
                    vec![]
                }
            };
            let search_text = collect_search_text(session, &entries);
            result.push(SessionWithText {
                session: session.clone(),
                search_text,
            });
        }

        Ok(result)
    }
}
