//! MemoryService — strategy layer, the single entry point for all callers.
//!
//! Does NOT depend on Engine, Message, PromptBuilder, or any kernel run types.
//! Accepts only plain data (strings, token counts).

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::kernel::memory::diagnostics;
use crate::kernel::memory::extractor::ExtractionResult;
use crate::kernel::memory::extractor::Extractor;
use crate::kernel::memory::store::MemoryEntry;
use crate::kernel::memory::store::MemoryScope;
use crate::kernel::memory::store::MemorySearchResult;
use crate::kernel::memory::store::MemoryStore;
use crate::llm::provider::LLMProvider;
use crate::types::Result;

/// The strategy-layer facade. Callers interact only with this.
pub struct MemoryService {
    store: Arc<dyn MemoryStore>,
    llm: Arc<dyn LLMProvider>,
    model: Arc<str>,
}

impl MemoryService {
    pub fn new(store: Arc<dyn MemoryStore>, llm: Arc<dyn LLMProvider>, model: Arc<str>) -> Self {
        Self { store, llm, model }
    }

    // ── Write path ──

    /// Extract facts from a plain-text transcript and persist them.
    /// Best-effort: failures are logged, never propagated.
    /// The `cancel` token ties extraction to the caller's lifecycle (e.g. a run).
    pub async fn extract_and_save(
        &self,
        transcript: &str,
        user_id: &str,
        agent_id: &str,
        cancel: CancellationToken,
    ) -> ExtractionResult {
        diagnostics::log_extract_started(user_id, agent_id);
        let extractor = Extractor::new(self.llm.clone(), self.model.clone(), cancel);
        let result = extractor
            .extract(transcript, user_id, agent_id, self.store.as_ref())
            .await;
        diagnostics::log_extract_done(user_id, result.facts_written);
        result
    }

    /// Save a single memory entry directly.
    pub async fn save(
        &self,
        user_id: &str,
        agent_id: &str,
        key: &str,
        content: &str,
        scope: MemoryScope,
    ) -> Result<()> {
        diagnostics::log_save(user_id, agent_id, key, &scope.to_string());
        let entry = MemoryEntry {
            id: crate::types::new_id(),
            user_id: user_id.to_string(),
            agent_id: agent_id.to_string(),
            scope,
            key: key.to_string(),
            content: content.to_string(),
            access_count: 0,
            last_accessed_at: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
        };
        self.store.write(&entry).await
    }

    // ── Read path ──

    /// Recall relevant memories as structured data. Caller decides formatting.
    pub async fn recall(&self, user_id: &str, agent_id: &str, limit: u32) -> Vec<MemoryEntry> {
        let entries = match self.store.list(user_id, agent_id, limit).await {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };
        diagnostics::log_recall(user_id, agent_id, entries.len(), 0);
        entries
    }

    /// FTS search for agent tool use.
    pub async fn search(
        &self,
        query: &str,
        user_id: &str,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<MemorySearchResult>> {
        let results = self.store.search(query, user_id, agent_id, limit).await?;
        // Fire-and-forget touch for accessed results
        for r in &results {
            let _ = self.store.touch(user_id, &r.id).await;
        }
        Ok(results)
    }

    // ── Maintenance path ──

    /// Prune stale memories. Returns count of pruned entries.
    pub async fn run_hygiene(
        &self,
        user_id: &str,
        max_age_days: u32,
        min_access: u32,
    ) -> Result<usize> {
        let pruned = self.store.prune(user_id, max_age_days, min_access).await?;
        diagnostics::log_hygiene(user_id, pruned);
        Ok(pruned)
    }
}
