use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use crate::error::Result;
use crate::storage::Storage;
use crate::types::ListTranscriptEntries;
use crate::types::SessionMeta;
use crate::types::TranscriptEntry;
use crate::types::TranscriptItem;

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

    /// Persist session meta (title, updated_at, context usage, etc.).
    pub async fn save(&self) -> Result<()> {
        let transcript = self.transcript.read().await;
        let mut meta = self.meta.write().await;
        // Build title from first + last user messages so the resume list
        // shows both the original topic and the most recent activity.
        if let Some(title) = build_title(&transcript) {
            meta.title = Some(title);
        }
        // Extract latest context window usage from compaction stats.
        if let Some((tokens, budget)) = last_context_usage(&transcript) {
            meta.context_tokens = tokens;
            meta.context_budget = budget;
        }
        meta.message_count = transcript.iter().filter(|i| i.is_context_item()).count() as u32;
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

/// Build a title from the first and last user messages.
///
/// - Single user message → that message (truncated to 56 chars).
/// - Multiple distinct messages → `head … tail` format.
fn build_title(items: &[TranscriptItem]) -> Option<String> {
    let user_texts: Vec<String> = items
        .iter()
        .filter_map(|item| {
            if let TranscriptItem::User { text } = item {
                let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
                if !normalized.is_empty() {
                    return Some(normalized);
                }
            }
            None
        })
        .collect();

    let first = user_texts.first()?;
    let last = user_texts.last()?;

    if first == last || user_texts.len() == 1 {
        // Single unique message — truncate to 56 chars
        let mut title: String = first.chars().take(56).collect();
        if first.chars().count() > 56 {
            title.push_str("...");
        }
        return Some(title);
    }

    // head … tail — split budget between first and last
    let max_half = 26;
    let mut head: String = first.chars().take(max_half).collect();
    if first.chars().count() > max_half {
        head.push_str("..");
    }
    let mut tail: String = last.chars().take(max_half).collect();
    if last.chars().count() > max_half {
        tail.push_str("..");
    }
    Some(format!("{head} … {tail}"))
}

/// Extract the latest context usage (estimated_tokens, budget_tokens) from
/// compaction-started stats entries in the transcript.
fn last_context_usage(items: &[TranscriptItem]) -> Option<(usize, usize)> {
    items.iter().rev().find_map(|item| {
        if let TranscriptItem::Stats { kind, data } = item {
            if kind == "context_compaction_started" {
                let tokens = data.get("estimated_tokens")?.as_u64()? as usize;
                let budget = data.get("budget_tokens")?.as_u64()? as usize;
                return Some((tokens, budget));
            }
        }
        None
    })
}

/// Build the conversation context view from raw transcript entries.
///
/// Finds the last `Compact` entry and uses its messages as the starting
/// point, then appends every entry that follows it. Items that are not
/// part of the conversation context (Stats, Compact itself) are filtered
/// out — they remain in the raw transcript.jsonl but never enter the
/// engine context.
fn resolve_transcript(entries: Vec<TranscriptEntry>) -> Vec<TranscriptItem> {
    let last_compact_idx = entries
        .iter()
        .rposition(|e| matches!(e.item, TranscriptItem::Compact { .. }));

    let mut items = match last_compact_idx {
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
    };

    // Project: keep only conversation context items.
    items.retain(|item| item.is_context_item());
    items
}
