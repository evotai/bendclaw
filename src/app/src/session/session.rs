use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::protocol::ListTranscriptEntries;
use crate::protocol::SessionMeta;
use crate::protocol::TranscriptEntry;
use crate::protocol::TranscriptItem;
use crate::storage::Storage;

pub struct Session {
    storage: Arc<dyn Storage>,
    meta: RwLock<SessionMeta>,
    transcript: RwLock<Vec<TranscriptItem>>,
}

impl Session {
    pub fn new(
        storage: Arc<dyn Storage>,
        meta: SessionMeta,
        transcript: Vec<TranscriptItem>,
    ) -> Arc<Self> {
        Arc::new(Self {
            storage,
            meta: RwLock::new(meta),
            transcript: RwLock::new(transcript),
        })
    }

    pub async fn create(
        session_id: String,
        cwd: String,
        model: String,
        storage: Arc<dyn Storage>,
    ) -> Result<Arc<Self>> {
        let meta = SessionMeta::new(session_id, cwd, model);
        storage.put_session(meta.clone()).await?;
        Ok(Self::new(storage, meta, Vec::new()))
    }

    pub async fn load(session_id: &str, storage: Arc<dyn Storage>) -> Result<Option<Arc<Self>>> {
        let meta = match storage.get_session(session_id).await? {
            Some(meta) => meta,
            None => return Ok(None),
        };

        let transcript = storage
            .list_transcript_entries(ListTranscriptEntries {
                session_id: session_id.to_string(),
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await?
            .into_iter()
            .map(|entry| entry.item)
            .collect();

        Ok(Some(Self::new(storage, meta, transcript)))
    }

    pub async fn set_model(&self, model: String) {
        self.meta.write().await.model = model;
    }

    pub async fn apply_transcript(&self, items: Vec<TranscriptItem>) {
        *self.transcript.write().await = items;

        let mut meta = self.meta.write().await;
        if meta
            .title
            .as_ref()
            .map(|title| title.trim().is_empty())
            .unwrap_or(true)
        {
            meta.title = first_user_title(&self.transcript.read().await);
        }
        meta.turns += 1;
        meta.updated_at = Utc::now().to_rfc3339();
    }

    pub async fn save(&self) -> Result<()> {
        let meta = self.meta().await;
        let items = self.transcript().await;

        let entries = items
            .into_iter()
            .enumerate()
            .map(|(idx, item)| {
                TranscriptEntry::new(
                    meta.session_id.clone(),
                    None,
                    idx as u64 + 1,
                    meta.turns,
                    item,
                )
            })
            .collect();

        self.storage.put_session(meta).await?;
        self.storage.put_transcript_entries(entries).await?;
        Ok(())
    }

    pub async fn meta(&self) -> SessionMeta {
        self.meta.read().await.clone()
    }

    pub async fn transcript(&self) -> Vec<TranscriptItem> {
        self.transcript.read().await.clone()
    }

    pub async fn session_id(&self) -> String {
        self.meta.read().await.session_id.clone()
    }
}

fn first_user_title(items: &[TranscriptItem]) -> Option<String> {
    let text = items.iter().find_map(|item| {
        if let TranscriptItem::User { text } = item {
            if !text.trim().is_empty() {
                return Some(text.clone());
            }
        }
        None
    })?;

    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    let mut title: String = normalized.chars().take(56).collect();
    if normalized.chars().count() > 56 {
        title.push_str("...");
    }
    Some(title)
}
