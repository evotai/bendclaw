use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::agent::ListTranscriptEntries;
use crate::agent::SessionMeta;
use crate::agent::TranscriptEntry;
use crate::agent::TranscriptItem;
use crate::error::Result;
use crate::storage::Storage;

pub struct Session {
    storage: Arc<dyn Storage>,
    meta: RwLock<SessionMeta>,
    transcript: RwLock<Vec<TranscriptItem>>,
    next_seq: RwLock<u64>,
}

impl Session {
    fn init(
        storage: Arc<dyn Storage>,
        meta: SessionMeta,
        transcript: Vec<TranscriptItem>,
        next_seq: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            storage,
            meta: RwLock::new(meta),
            transcript: RwLock::new(transcript),
            next_seq: RwLock::new(next_seq),
        })
    }

    pub async fn new(
        session_id: String,
        cwd: String,
        model: String,
        storage: Arc<dyn Storage>,
    ) -> Result<Arc<Self>> {
        let meta = SessionMeta::new(session_id, cwd, model);
        storage.save_session(meta.clone()).await?;
        Ok(Self::init(storage, meta, Vec::new(), 0))
    }

    pub async fn open(session_id: &str, storage: Arc<dyn Storage>) -> Result<Option<Arc<Self>>> {
        let meta = match storage.get_session(session_id).await? {
            Some(meta) => meta,
            None => return Ok(None),
        };

        let entries = storage
            .list_entries(ListTranscriptEntries {
                session_id: session_id.to_string(),
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await?;

        let next_seq = entries.last().map(|e| e.seq).unwrap_or(0);
        let transcript = resolve_transcript(entries);

        Ok(Some(Self::init(storage, meta, transcript, next_seq)))
    }

    pub async fn set_model(&self, model: String) {
        self.meta.write().await.model = model;
    }

    /// Append items to the transcript and persist them (append-only).
    pub async fn write_items(&self, items: Vec<TranscriptItem>) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        let session_id = self.meta.read().await.session_id.clone();
        let turn = self.meta.read().await.turns;

        {
            let mut transcript = self.transcript.write().await;
            transcript.extend(items.iter().cloned());
        }

        for item in &items {
            let seq = self.next_seq().await;
            let entry = TranscriptEntry::new(session_id.clone(), None, seq, turn, item.clone());
            self.storage.append_entry(entry).await?;
        }
        Ok(())
    }

    /// Increment the turn counter. Call once per real conversation turn.
    pub async fn increment_turn(&self) {
        self.meta.write().await.turns += 1;
    }

    /// Persist session meta (title, updated_at, etc.).
    pub async fn save(&self) -> Result<()> {
        let transcript = self.transcript.read().await;
        let mut meta = self.meta.write().await;
        if meta
            .title
            .as_ref()
            .map(|title| title.trim().is_empty())
            .unwrap_or(true)
        {
            meta.title = first_user_title(&transcript);
        }
        meta.updated_at = Utc::now().to_rfc3339();
        self.storage.save_session(meta.clone()).await
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

    async fn next_seq(&self) -> u64 {
        let mut s = self.next_seq.write().await;
        *s += 1;
        *s
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

/// Find the last `Compact` entry in the raw transcript log and use its
/// `messages` as the starting point, then append every entry that follows it.
/// If no `Compact` entry exists, all entries are returned as-is.
fn resolve_transcript(entries: Vec<TranscriptEntry>) -> Vec<TranscriptItem> {
    let last_compact_idx = entries
        .iter()
        .rposition(|e| matches!(e.item, TranscriptItem::Compact { .. }));

    match last_compact_idx {
        Some(idx) => {
            let compact = &entries[idx];
            let mut items = match &compact.item {
                TranscriptItem::Compact { messages } => messages.clone(),
                _ => Vec::new(),
            };
            for entry in &entries[idx + 1..] {
                items.push(entry.item.clone());
            }
            items
        }
        None => entries.into_iter().map(|e| e.item).collect(),
    }
}
