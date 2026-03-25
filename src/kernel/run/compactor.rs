use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use crate::kernel::run::checkpoint::CompactionCheckpoint;
use crate::kernel::run::compaction_rules::plan_compaction_split;
use crate::kernel::Message;
use crate::kernel::OpType;
use crate::kernel::OperationMeta;
use crate::llm::message::ChatMessage;
use crate::llm::provider::LLMProvider;
use crate::llm::tokens::count_tokens;
use crate::llm::usage::TokenUsage;
use crate::observability::log::slog;

/// Maximum characters per chunk for staged summarization (~10K tokens).
const CHUNK_SIZE: usize = 40_000;

/// Minimum interval between compaction attempts.
const COMPACTION_COOLDOWN: Duration = Duration::from_secs(60);

/// Metadata returned when compaction occurs.
pub struct CompactionResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub summary_len: usize,
    /// Optional persistent checkpoint derived from the compaction.
    pub checkpoint: Option<CompactionCheckpoint>,
    /// Tokens consumed by compaction LLM calls.
    pub token_usage: TokenUsage,
    /// Duration of the compaction in milliseconds.
    pub duration_ms: u64,
}

/// LLM-powered context compactor.
///
/// Workflow:
/// 1. Summarize: LLM summarizes old messages
/// 2. Truncate: Replace old messages with summary
pub struct Compactor {
    llm: Arc<dyn LLMProvider>,
    model: Arc<str>,
    cancel: CancellationToken,
    compaction_failures: u32,
    last_compaction_at: Option<Instant>,
    last_error: Option<String>,
}

impl Compactor {
    pub fn new(llm: Arc<dyn LLMProvider>, model: Arc<str>, cancel: CancellationToken) -> Self {
        Self {
            llm,
            model,
            cancel,
            compaction_failures: 0,
            last_compaction_at: None,
            last_error: None,
        }
    }

    /// Compact the message list when estimated tokens exceed `max_context_tokens`.
    ///
    /// Returns `Some(CompactionResult)` when compaction occurred, `None` otherwise.
    pub async fn compact(
        &mut self,
        messages: &mut Vec<Message>,
        max_context_tokens: usize,
        current_run_id: &str,
    ) -> Option<CompactionResult> {
        let compact_tracker = OperationMeta::begin(OpType::Compaction);
        let compact_start = compact_tracker.start_time();
        let messages_before = messages.len();

        let msg_tokens: Vec<usize> = messages.iter().map(|m| count_tokens(&m.text())).collect();
        let total_tokens: usize = msg_tokens.iter().sum();

        // Skip compaction if too many consecutive failures
        if self.compaction_failures >= 3 {
            slog!(
                warn,
                "compaction",
                "skipped",
                failures = self.compaction_failures,
                last_error = self.last_error.as_deref().unwrap_or("unknown"),
            );
            return None;
        }

        // Check if compaction needed
        if total_tokens <= max_context_tokens {
            return None;
        }

        slog!(
            info,
            "compaction",
            "triggered",
            total_tokens,
            max_context_tokens,
        );

        // Cooldown: skip expensive summarization if recent compaction was ineffective
        if self.compaction_failures > 0 {
            if let Some(last) = self.last_compaction_at {
                if last.elapsed() < COMPACTION_COOLDOWN {
                    slog!(
                        info,
                        "compaction",
                        "cooldown_active",
                        elapsed_secs = last.elapsed().as_secs(),
                        failures = self.compaction_failures,
                    );
                    return None;
                }
            }
        }

        // 5. Find split point: keep tail messages within budget
        let plan = plan_compaction_split(messages, &msg_tokens, max_context_tokens)?;
        let split = plan.split_index;

        // 5. Partition: system messages kept, non-system split into dropped/kept
        let (system, non_system): (Vec<Message>, Vec<Message>) =
            messages.iter().cloned().partition(|m| {
                matches!(
                    m,
                    Message::System { .. } | Message::CompactionSummary { .. }
                )
            });

        // Map split index from full messages to non_system index
        let non_system_split = {
            let mut ns_idx = 0;
            let mut full_idx = 0;
            for msg in messages.iter() {
                if matches!(
                    msg,
                    Message::System { .. } | Message::CompactionSummary { .. }
                ) {
                    full_idx += 1;
                    continue;
                }
                if full_idx >= split {
                    break;
                }
                ns_idx += 1;
                full_idx += 1;
            }
            ns_idx
        };

        if non_system_split == 0 {
            return None;
        }

        let dropped: Vec<&Message> = non_system.iter().take(non_system_split).collect();
        let (summary, token_usage) = self.summarize(&dropped).await;

        if summary.is_none() {
            self.last_error = Some("summarization returned no content".to_string());
        }

        let summary_len = summary.as_ref().map(|s| s.len()).unwrap_or(0);

        let mut compacted = system;
        let mut checkpoint = None;
        if let Some(text) = summary {
            checkpoint = build_checkpoint(&dropped, current_run_id, &text);
            let meta = compact_tracker
                .summary(format!(
                    "{} -> {} messages, {} -> ~{} tokens",
                    messages_before,
                    messages_before - non_system_split,
                    total_tokens,
                    plan.kept_tokens + crate::kernel::run::compaction_rules::SUMMARY_RESERVE,
                ))
                .finish();
            compacted.push(Message::compaction_with_operation(text, meta));
        }
        compacted.extend(non_system.into_iter().skip(non_system_split));

        let messages_after = compacted.len();
        *messages = compacted;

        if messages_after >= messages_before {
            self.compaction_failures += 1;
            self.last_error = Some(format!(
                "compaction did not reduce: {messages_before} -> {messages_after}"
            ));
            slog!(
                warn,
                "compaction",
                "ineffective",
                messages_before,
                messages_after,
                consecutive_failures = self.compaction_failures,
            );
        } else {
            self.compaction_failures = 0;
        }

        // Token-level effectiveness check: if tokens barely dropped, apply cooldown
        let post_tokens: usize = messages.iter().map(|m| count_tokens(&m.text())).sum();
        if post_tokens > total_tokens * 9 / 10 {
            self.compaction_failures += 1;
            self.last_compaction_at = Some(Instant::now());
            slog!(
                warn,
                "compaction",
                "tokens_barely_reduced",
                pre_tokens = total_tokens,
                post_tokens,
            );
        }

        Some(CompactionResult {
            messages_before,
            messages_after,
            summary_len,
            checkpoint,
            token_usage,
            duration_ms: compact_start.elapsed().as_millis() as u64,
        })
    }

    async fn summarize(&self, dropped: &[&Message]) -> (Option<String>, TokenUsage) {
        let transcript = Self::build_transcript(dropped);
        if transcript.is_empty() {
            return (None, TokenUsage::default());
        }

        let chunks = Self::split_chunks(&transcript, CHUNK_SIZE);

        if chunks.len() <= 1 {
            return self.summarize_single(&transcript).await;
        }

        let mut total_usage = TokenUsage::default();
        let mut partial_summaries = Vec::new();
        for chunk in &chunks {
            let (text, usage) = self.summarize_single(chunk).await;
            total_usage += &usage;
            if let Some(t) = text {
                partial_summaries.push(t);
            }
        }

        let merged = partial_summaries.join("\n\n");
        if merged.is_empty() {
            return (None, total_usage);
        }

        let (text, merge_usage) = self.summarize_single(&merged).await;
        total_usage += &merge_usage;
        (text, total_usage)
    }

    async fn summarize_single(&self, text: &str) -> (Option<String>, TokenUsage) {
        let prompt = format!(
            "Summarize the following conversation excerpt in 2-4 concise paragraphs. \
             Focus on: key decisions, tool calls, important facts, unresolved questions. \
             No greetings or filler.\n\n{text}"
        );

        let messages = vec![ChatMessage::user(prompt)];

        tokio::select! {
            result = self.llm.chat(&self.model, &messages, &[], 0.0) => {
                match result {
                    Ok(resp) => {
                        let usage = resp.usage.unwrap_or_default();
                        let content = resp.content.filter(|s| !s.is_empty());
                        (content, usage)
                    }
                    Err(e) => {
                        slog!(warn, "compaction", "summarize_failed", error = %e,);
                        (None, TokenUsage::default())
                    }
                }
            }
            _ = self.cancel.cancelled() => {

                (None, TokenUsage::default())
            }
        }
    }

    fn build_transcript(dropped: &[&Message]) -> String {
        let mut transcript = String::new();
        for msg in dropped {
            let role = msg.role().map(|r| r.to_string()).unwrap_or("note".into());
            let text = msg.text();
            if !text.is_empty() {
                transcript.push_str(&format!("[{role}]: {text}\n\n"));
            }
        }
        transcript
    }

    pub fn split_chunks(text: &str, max_chars: usize) -> Vec<&str> {
        if text.len() <= max_chars {
            return vec![text];
        }

        let mut chunks = Vec::new();
        let mut start = 0;

        while start < text.len() {
            let end = (start + max_chars).min(text.len());
            if end == text.len() {
                chunks.push(&text[start..]);
                break;
            }

            let slice = &text[start..end];
            let break_at = slice
                .rfind("\n\n")
                .map(|pos| start + pos + 2)
                .unwrap_or(end);

            chunks.push(&text[start..break_at]);
            start = break_at;
        }

        chunks
    }
}

fn build_checkpoint(
    dropped: &[&Message],
    current_run_id: &str,
    summary_text: &str,
) -> Option<CompactionCheckpoint> {
    let mut last_run_id = None;
    for msg in dropped {
        let run_id = msg.origin_run_id()?;
        if run_id == current_run_id {
            return None;
        }
        last_run_id = Some(run_id);
    }

    last_run_id.map(|through_run_id| CompactionCheckpoint {
        summary_text: summary_text.to_string(),
        through_run_id: through_run_id.to_string(),
    })
}
