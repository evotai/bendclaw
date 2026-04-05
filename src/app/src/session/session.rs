use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::storage::model::ListTranscriptEntries;
use crate::storage::model::SessionMeta;
use crate::storage::model::TranscriptEntry;
use crate::storage::Storage;

pub struct Session {
    storage: Arc<dyn Storage>,
    meta: RwLock<SessionMeta>,
    messages: RwLock<Vec<bend_agent::Message>>,
}

impl Session {
    pub fn new(
        storage: Arc<dyn Storage>,
        meta: SessionMeta,
        messages: Vec<bend_agent::Message>,
    ) -> Arc<Self> {
        Arc::new(Self {
            storage,
            meta: RwLock::new(meta),
            messages: RwLock::new(messages),
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

        let messages = storage
            .list_transcript_entries(ListTranscriptEntries {
                session_id: session_id.to_string(),
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await?
            .into_iter()
            .map(|entry| entry.message)
            .collect();

        Ok(Some(Self::new(storage, meta, messages)))
    }

    pub async fn set_model(&self, model: String) {
        self.meta.write().await.model = model;
    }

    pub async fn apply_messages(&self, messages: Vec<bend_agent::Message>) {
        *self.messages.write().await = messages;

        let mut meta = self.meta.write().await;
        if meta
            .title
            .as_ref()
            .map(|title| title.trim().is_empty())
            .unwrap_or(true)
        {
            meta.title = first_user_title(&self.messages.read().await);
        }
        meta.turns += 1;
        meta.updated_at = Utc::now().to_rfc3339();
    }

    pub async fn save(&self) -> Result<()> {
        let meta = self.meta().await;
        let messages = self.messages().await;

        let entries = messages
            .into_iter()
            .enumerate()
            .map(|(idx, message)| {
                TranscriptEntry::new(
                    meta.session_id.clone(),
                    None,
                    idx as u64 + 1,
                    meta.turns,
                    message,
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

    pub async fn messages(&self) -> Vec<bend_agent::Message> {
        self.messages.read().await.clone()
    }

    pub async fn session_id(&self) -> String {
        self.meta.read().await.session_id.clone()
    }
}

fn first_user_title(messages: &[bend_agent::Message]) -> Option<String> {
    let text = messages
        .iter()
        .find(|message| message.role == bend_agent::MessageRole::User)
        .map(bend_agent::types::extract_text)?
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if text.is_empty() {
        return None;
    }

    let mut title: String = text.chars().take(56).collect();
    if text.chars().count() > 56 {
        title.push_str("...");
    }
    Some(title)
}
