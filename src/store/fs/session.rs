use std::path::PathBuf;

use async_trait::async_trait;
use tokio::fs;

use crate::error::BendclawError;
use crate::error::Result;
use crate::session::SessionMeta;
use crate::store::session::SessionStore;

pub struct FsSessionStore {
    base_dir: PathBuf,
}

impl FsSessionStore {
    pub fn new(base_dir: PathBuf) -> Self {
        Self { base_dir }
    }

    fn meta_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{session_id}.json"))
    }

    fn transcript_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{session_id}-transcript.json"))
    }
}

#[async_trait]
impl SessionStore for FsSessionStore {
    async fn save_meta(&self, meta: &SessionMeta) -> Result<()> {
        fs::create_dir_all(&self.base_dir).await?;
        let json = serde_json::to_string_pretty(meta)?;
        fs::write(self.meta_path(&meta.session_id), json).await?;
        Ok(())
    }

    async fn load_meta(&self, session_id: &str) -> Result<Option<SessionMeta>> {
        let path = self.meta_path(session_id);
        match fs::read_to_string(&path).await {
            Ok(content) => {
                let meta: SessionMeta = serde_json::from_str(&content)?;
                Ok(Some(meta))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BendclawError::Io(e)),
        }
    }

    async fn list_recent(&self, limit: usize) -> Result<Vec<SessionMeta>> {
        let mut entries = match fs::read_dir(&self.base_dir).await {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(BendclawError::Io(e)),
        };

        let mut metas: Vec<SessionMeta> = Vec::new();
        while let Some(entry) = entries.next_entry().await? {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.ends_with(".json") && !name.ends_with("-transcript.json") {
                let path = entry.path();
                if let Ok(content) = fs::read_to_string(&path).await {
                    if let Ok(meta) = serde_json::from_str::<SessionMeta>(&content) {
                        metas.push(meta);
                    }
                }
            }
        }

        metas.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        metas.truncate(limit);
        Ok(metas)
    }

    async fn save_transcript(
        &self,
        session_id: &str,
        messages: &[bend_agent::Message],
    ) -> Result<()> {
        fs::create_dir_all(&self.base_dir).await?;
        let json = serde_json::to_string_pretty(messages)?;
        fs::write(self.transcript_path(session_id), json).await?;
        Ok(())
    }

    async fn load_transcript(&self, session_id: &str) -> Result<Option<Vec<bend_agent::Message>>> {
        let path = self.transcript_path(session_id);
        match fs::read_to_string(&path).await {
            Ok(content) => {
                let messages: Vec<bend_agent::Message> = serde_json::from_str(&content)?;
                Ok(Some(messages))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(BendclawError::Io(e)),
        }
    }
}
