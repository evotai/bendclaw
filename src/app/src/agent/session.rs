use std::sync::Arc;

use chrono::Utc;
use tokio::sync::RwLock;

use super::session_locator::SessionLocator;
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
        Self::new_with_source(session_id, cwd, model, "", storage).await
    }

    pub async fn new_with_source(
        session_id: String,
        cwd: String,
        model: String,
        source: &str,
        storage: Arc<dyn Storage>,
    ) -> Result<Arc<Self>> {
        let meta = SessionMeta::new(session_id, cwd, model).with_source(source);
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

    /// Open an existing session by locator, or create a new one.
    /// This is the single entry point for all channel-based session resolution.
    pub async fn open_or_create(
        locator: &SessionLocator,
        cwd: &str,
        model: &str,
        storage: Arc<dyn Storage>,
    ) -> Result<Arc<Self>> {
        let id = locator.session_id();
        match Self::open(&id, storage.clone()).await? {
            Some(session) => {
                session.set_model(model.to_string()).await;
                Ok(session)
            }
            None => {
                Self::new_with_source(
                    id,
                    cwd.to_string(),
                    model.to_string(),
                    &locator.stable_key(),
                    storage,
                )
                .await
            }
        }
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

    // -- marker methods -------------------------------------------------------

    /// Write a `/clear` marker — resets context to empty.
    pub async fn write_clear_marker(&self) -> Result<()> {
        let item = TranscriptItem::Marker {
            kind: crate::types::MarkerKind::Clear,
            target_seq: None,
            messages: vec![],
        };
        self.write_items(vec![item]).await?;
        // Replace in-memory transcript with the empty baseline.
        *self.transcript.write().await = vec![];
        Ok(())
    }

    /// Write a `/goto` marker — resets context to the snapshot at `target_seq`.
    pub async fn write_goto_marker(&self, target_seq: u64) -> Result<()> {
        let entries = self.load_all_entries().await?;
        let snapshot = resolve_snapshot_at(&entries, target_seq);
        let item = TranscriptItem::Marker {
            kind: crate::types::MarkerKind::Goto,
            target_seq: Some(target_seq),
            messages: snapshot.clone(),
        };
        self.write_items(vec![item]).await?;
        // Replace in-memory transcript with the restored baseline.
        *self.transcript.write().await = snapshot;
        Ok(())
    }

    /// Write a compaction marker — resets context to the compacted snapshot.
    pub async fn write_compact_marker(&self, messages: Vec<TranscriptItem>) -> Result<()> {
        let item = TranscriptItem::Marker {
            kind: crate::types::MarkerKind::Compact,
            target_seq: None,
            messages: messages.clone(),
        };
        self.write_items(vec![item]).await?;
        *self.transcript.write().await = messages;
        Ok(())
    }

    /// Check whether `seq` points to a valid goto-able message in storage.
    pub async fn is_valid_context_seq(&self, seq: u64) -> Result<bool> {
        let entries = self.load_all_entries().await?;
        Ok(entries
            .iter()
            .any(|e| e.seq == seq && is_goto_target(&e.item)))
    }

    /// Get the transcript item at a specific seq number.
    pub async fn get_item_at(&self, seq: u64) -> Result<Option<TranscriptItem>> {
        let entries = self.load_all_entries().await?;
        Ok(entries.into_iter().find(|e| e.seq == seq).map(|e| e.item))
    }

    /// Current max seq number.
    pub async fn max_seq(&self) -> u64 {
        *self.next_seq.read().await
    }

    /// Load all raw transcript entries from storage.
    pub async fn load_all_entries(&self) -> Result<Vec<TranscriptEntry>> {
        let session_id = self.meta.read().await.session_id.clone();
        self.storage
            .list_entries(ListTranscriptEntries {
                session_id,
                run_id: None,
                after_seq: None,
                limit: None,
            })
            .await
    }

    /// Return the most recent user/assistant messages visible in the current context.
    ///
    /// - Snapshot baseline messages have `seq = 0` (not addressable by `/goto`).
    /// - Post-marker messages have their real `seq` (addressable by `/goto`).
    /// - Empty messages are excluded.
    pub async fn recent_context_entries(&self, limit: usize) -> Result<Vec<(u64, TranscriptItem)>> {
        let entries = self.load_all_entries().await?;

        let last_control = entries.iter().rposition(|e| is_control_point(&e.item));

        let mut context: Vec<(u64, TranscriptItem)> = Vec::new();

        match last_control {
            Some(idx) => {
                // Snapshot baseline — seq=0 means not addressable by /goto
                if let Some(snapshot) = extract_snapshot(&entries[idx].item) {
                    for item in snapshot {
                        if is_history_visible(&item) {
                            context.push((0, item));
                        }
                    }
                }
                // Post-marker entries with real seq
                for entry in &entries[idx + 1..] {
                    if is_history_visible(&entry.item) {
                        context.push((entry.seq, entry.item.clone()));
                    }
                }
            }
            None => {
                for entry in &entries {
                    if is_history_visible(&entry.item) {
                        context.push((entry.seq, entry.item.clone()));
                    }
                }
            }
        }

        let start = context.len().saturating_sub(limit);
        Ok(context[start..].to_vec())
    }
}

/// Whether an item should appear in `/history` output.
/// Non-empty user and assistant messages are shown, but only user messages are
/// addressable by `/goto` (see [`is_goto_target`]).
fn is_history_visible(item: &TranscriptItem) -> bool {
    match item {
        TranscriptItem::User { text, .. } => !text.trim().is_empty(),
        TranscriptItem::Assistant { text, .. } => !text.trim().is_empty(),
        _ => false,
    }
}

/// Whether an item is a valid `/goto` target — only non-empty user messages.
fn is_goto_target(item: &TranscriptItem) -> bool {
    matches!(item, TranscriptItem::User { text, .. } if !text.trim().is_empty())
}

/// Build a title from the first and last user messages.
///
/// - Single user message → that message (truncated to 56 chars).
/// - Multiple distinct messages → `head … tail` format.
fn build_title(items: &[TranscriptItem]) -> Option<String> {
    let user_texts: Vec<String> = items
        .iter()
        .filter_map(|item| {
            if let TranscriptItem::User { text, .. } = item {
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

/// Extract the context snapshot from a control-point item.
/// Returns `Some(messages)` for old `Compact` and new `Marker` variants.
fn extract_snapshot(item: &TranscriptItem) -> Option<Vec<TranscriptItem>> {
    match item {
        TranscriptItem::Compact { messages } => Some(messages.clone()),
        TranscriptItem::Marker { messages, .. } => Some(messages.clone()),
        _ => None,
    }
}

/// Returns true if this item is a control point (Compact or Marker).
fn is_control_point(item: &TranscriptItem) -> bool {
    matches!(
        item,
        TranscriptItem::Compact { .. } | TranscriptItem::Marker { .. }
    )
}

/// Build the conversation context view from raw transcript entries.
///
/// Finds the last control-point entry (old `Compact` or new `Marker`) and
/// uses its snapshot as the baseline, then appends every context item that
/// follows it. Items that are not part of the conversation context (Stats,
/// Compact, Marker) are filtered out.
fn resolve_transcript(entries: Vec<TranscriptEntry>) -> Vec<TranscriptItem> {
    let last_control = entries
        .iter()
        .rposition(|e| extract_snapshot(&e.item).is_some());

    match last_control {
        Some(idx) => {
            let mut items = extract_snapshot(&entries[idx].item).unwrap_or_default();
            for entry in &entries[idx + 1..] {
                if entry.item.is_context_item() {
                    items.push(entry.item.clone());
                }
            }
            items
        }
        None => entries
            .iter()
            .filter(|e| e.item.is_context_item())
            .map(|e| e.item.clone())
            .collect(),
    }
}

/// Resolve the context snapshot as it was at `target_seq`.
/// Used by `/goto` to compute the baseline at a historical point.
fn resolve_snapshot_at(entries: &[TranscriptEntry], target_seq: u64) -> Vec<TranscriptItem> {
    let scoped: Vec<TranscriptEntry> = entries
        .iter()
        .filter(|e| e.seq <= target_seq)
        .cloned()
        .collect();
    resolve_transcript(scoped)
}
