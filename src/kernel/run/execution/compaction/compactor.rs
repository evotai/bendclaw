//! Compactor — stateful coordinator for context compaction.
//!
//! Owns failure tracking, cooldown logic, and delegates the actual
//! message transformation to a `CompactionStrategy`.

use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio_util::sync::CancellationToken;

use super::diagnostics;
use super::strategy::CompactionConfig;
use super::strategy::CompactionStrategy;
use super::tiered::TieredCompactionStrategy;
use crate::kernel::run::checkpoint::CompactionCheckpoint;
use crate::kernel::run::prompt_projection;
use crate::kernel::Message;
use crate::llm::provider::LLMProvider;
use crate::llm::usage::TokenUsage;

/// Minimum interval between compaction attempts.
const COMPACTION_COOLDOWN: Duration = Duration::from_secs(60);

/// Metadata returned when compaction occurs.
pub struct CompactionResult {
    pub messages_before: usize,
    pub messages_after: usize,
    pub summary_len: usize,
    pub checkpoint: Option<CompactionCheckpoint>,
    pub token_usage: TokenUsage,
    pub duration_ms: u64,
}

/// Stateful compaction coordinator.
///
/// Delegates the actual compaction to a [`CompactionStrategy`].
/// Manages failure tracking and cooldown.
pub struct Compactor {
    strategy: Box<dyn CompactionStrategy>,
    config: CompactionConfig,
    compaction_failures: u32,
    last_compaction_at: Option<Instant>,
    last_error: Option<String>,
}

impl Compactor {
    /// Backward-compatible constructor: uses `TieredCompactionStrategy`.
    pub fn new(llm: Arc<dyn LLMProvider>, model: Arc<str>, cancel: CancellationToken) -> Self {
        let strategy = Box::new(TieredCompactionStrategy::new(llm, model, cancel));
        Self {
            strategy,
            config: CompactionConfig::default(),
            compaction_failures: 0,
            last_compaction_at: None,
            last_error: None,
        }
    }

    /// Constructor with a custom strategy and config.
    pub fn with_strategy(strategy: Box<dyn CompactionStrategy>, config: CompactionConfig) -> Self {
        Self {
            strategy,
            config,
            compaction_failures: 0,
            last_compaction_at: None,
            last_error: None,
        }
    }

    /// Compact the message list when estimated prompt tokens exceed budget.
    ///
    /// Returns `Some(CompactionResult)` when compaction occurred, `None` otherwise.
    pub async fn compact(
        &mut self,
        messages: &mut Vec<Message>,
        max_context_tokens: usize,
        current_run_id: &str,
    ) -> Option<CompactionResult> {
        let start = Instant::now();
        let messages_before = messages.len();

        let prompt_tokens = prompt_projection::count_prompt_tokens(messages);

        // Skip if too many consecutive failures
        if self.compaction_failures >= 3 {
            diagnostics::log_compaction_skipped(
                self.compaction_failures,
                self.last_error.as_deref().unwrap_or("unknown"),
            );
            return None;
        }

        // Check if compaction needed
        if prompt_tokens <= max_context_tokens {
            return None;
        }

        diagnostics::log_compaction_triggered(prompt_tokens, max_context_tokens);

        // Cooldown: skip if recent compaction was ineffective
        if self.compaction_failures > 0 {
            if let Some(last) = self.last_compaction_at {
                if last.elapsed() < COMPACTION_COOLDOWN {
                    diagnostics::log_compaction_cooldown_active(
                        last.elapsed().as_secs(),
                        self.compaction_failures,
                    );
                    return None;
                }
            }
        }

        // Delegate to strategy
        let mut cfg = self.config.clone();
        cfg.max_context_tokens = max_context_tokens;
        let taken = std::mem::take(messages);
        let outcome = self
            .strategy
            .compact(taken.clone(), &cfg, current_run_id)
            .await;

        let outcome = match outcome {
            Some(o) => o,
            None => {
                *messages = taken;
                self.last_error = Some("strategy returned None".to_string());
                return None;
            }
        };

        let messages_after = outcome.messages.len();
        let token_usage = outcome.token_usage;
        let checkpoint = outcome.checkpoint;
        *messages = outcome.messages;

        // Compute summary_len from any CompactionSummary in the result
        let summary_len = messages
            .iter()
            .filter_map(|m| match m {
                Message::CompactionSummary { summary, .. } => Some(summary.len()),
                _ => None,
            })
            .next_back()
            .unwrap_or(0);

        // Effectiveness check: message count
        if messages_after >= messages_before {
            self.compaction_failures += 1;
            self.last_error = Some(format!(
                "compaction did not reduce: {messages_before} -> {messages_after}"
            ));
            diagnostics::log_compaction_ineffective(
                messages_before,
                messages_after,
                self.compaction_failures,
            );
        } else {
            self.compaction_failures = 0;
        }

        // Effectiveness check: token count
        let post_tokens = prompt_projection::count_prompt_tokens(messages);
        if post_tokens > prompt_tokens * 9 / 10 {
            self.compaction_failures += 1;
            self.last_compaction_at = Some(Instant::now());
            diagnostics::log_compaction_tokens_barely_reduced(prompt_tokens, post_tokens);
        }

        Some(CompactionResult {
            messages_before,
            messages_after,
            summary_len,
            checkpoint,
            token_usage,
            duration_ms: start.elapsed().as_millis() as u64,
        })
    }
}
