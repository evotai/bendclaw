//! Memory extraction — LLM-powered fact extraction from conversation text.

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::kernel::memory::store::MemoryEntry;
use crate::kernel::memory::store::MemoryScope;
use crate::kernel::memory::store::MemoryStore;
use crate::llm::message::ChatMessage;
use crate::llm::provider::LLMProvider;
use crate::llm::usage::TokenUsage;
use crate::observability::log::slog;
use crate::types::Result;

/// Maximum facts to extract per call (prevent noise).
const MAX_FACTS: usize = 10;

/// Maximum transcript chars sent to LLM.
const MAX_TRANSCRIPT_CHARS: usize = 40_000;

/// Result of an extraction attempt.
#[derive(Debug, Clone)]
pub struct ExtractionResult {
    pub facts_written: usize,
    pub token_usage: TokenUsage,
}

/// Extracts key facts from conversation text via LLM.
pub struct Extractor {
    llm: Arc<dyn LLMProvider>,
    model: Arc<str>,
    cancel: CancellationToken,
}

impl Extractor {
    pub fn new(llm: Arc<dyn LLMProvider>, model: Arc<str>, cancel: CancellationToken) -> Self {
        Self { llm, model, cancel }
    }

    /// Extract facts from transcript and write to store. Best-effort.
    pub async fn extract(
        &self,
        transcript: &str,
        user_id: &str,
        agent_id: &str,
        store: &dyn MemoryStore,
    ) -> ExtractionResult {
        if transcript.trim().is_empty() {
            return ExtractionResult {
                facts_written: 0,
                token_usage: TokenUsage::default(),
            };
        }

        let truncated = truncate_transcript(transcript);
        let (json_text, usage) = match self.call_llm(truncated).await {
            Some(r) => r,
            None => {
                return ExtractionResult {
                    facts_written: 0,
                    token_usage: TokenUsage::default(),
                }
            }
        };

        let facts = match parse_facts(&json_text) {
            Ok(f) => f,
            Err(e) => {
                slog!(
                    warn,
                    "memory.extractor",
                    "parse_failed",
                    error = e.to_string(),
                );
                return ExtractionResult {
                    facts_written: 0,
                    token_usage: usage,
                };
            }
        };

        let mut written = 0;
        for fact in facts.into_iter().take(MAX_FACTS) {
            let entry = MemoryEntry {
                id: crate::types::new_id(),
                user_id: user_id.to_string(),
                agent_id: agent_id.to_string(),
                scope: parse_fact_scope(&fact.scope),
                key: fact.key,
                content: fact.content,
                access_count: 0,
                last_accessed_at: String::new(),
                created_at: String::new(),
                updated_at: String::new(),
            };
            if store.write(&entry).await.is_ok() {
                written += 1;
            }
        }

        slog!(
            info,
            "memory.extractor",
            "extracted",
            facts_written = written,
        );
        ExtractionResult {
            facts_written: written,
            token_usage: usage,
        }
    }

    async fn call_llm(&self, transcript: &str) -> Option<(String, TokenUsage)> {
        let prompt = format!(
            "Extract key facts from this conversation that are worth remembering long-term.\n\
             Focus on: user preferences, decisions, important facts, project context.\n\
             Skip: greetings, filler, transient details.\n\n\
             Return a JSON array (max {MAX_FACTS} items):\n\
             [{{\"key\": \"short_identifier\", \"content\": \"the fact\", \"scope\": \"agent|shared\"}}]\n\n\
             Use scope \"shared\" for facts useful to all agents (user preferences, org info).\n\
             Use scope \"agent\" for facts specific to this conversation context.\n\
             If nothing worth saving, return [].\n\n\
             Conversation:\n{transcript}"
        );

        let messages = vec![ChatMessage::user(prompt)];

        tokio::select! {
            result = self.llm.chat(&self.model, &messages, &[], 0.0) => {
                match result {
                    Ok(resp) => {
                        let usage = resp.usage.unwrap_or_default();
                        resp.content.map(|c| (c, usage))
                    }
                    Err(e) => {
                        slog!(warn, "memory.extractor", "llm_failed", error = e.to_string(),);
                        None
                    }
                }
            }
            _ = self.cancel.cancelled() => None,
        }
    }
}

// ── Parsing ──

#[derive(Debug, serde::Deserialize)]
struct RawFact {
    key: String,
    content: String,
    #[serde(default = "default_scope")]
    scope: String,
}

fn default_scope() -> String {
    "agent".to_string()
}

fn parse_fact_scope(s: &str) -> MemoryScope {
    match s {
        "shared" => MemoryScope::Shared,
        _ => MemoryScope::Agent,
    }
}

fn parse_facts(text: &str) -> Result<Vec<RawFact>> {
    // Find JSON array in response (LLM may wrap in markdown code block)
    let trimmed = text.trim();
    let json_str = if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            &trimmed[start..=end]
        } else {
            trimmed
        }
    } else {
        trimmed
    };

    let facts: Vec<RawFact> = serde_json::from_str(json_str).map_err(|e| {
        crate::types::ErrorCode::internal(format!("failed to parse extraction JSON: {e}"))
    })?;

    Ok(facts
        .into_iter()
        .filter(|f| !f.key.is_empty() && !f.content.is_empty())
        .collect())
}

fn truncate_transcript(text: &str) -> &str {
    if text.len() <= MAX_TRANSCRIPT_CHARS {
        return text;
    }
    let mut end = MAX_TRANSCRIPT_CHARS;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
