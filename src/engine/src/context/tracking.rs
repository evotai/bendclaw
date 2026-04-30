//! Context tracking, configuration, and execution limits.

use serde::Deserialize;
use serde::Serialize;

use super::tokens::message_tokens;
use super::tokens::total_tokens;
use crate::types::*;

// ---------------------------------------------------------------------------
// Context tracking (real usage + estimates)
// ---------------------------------------------------------------------------

/// Tracks context size using provider usage as baseline, with chars/4
/// fallback. Single authority for context budget estimation — both
/// compaction and LLM call events source from this tracker.
pub struct ContextTracker {
    last_baseline_tokens: Option<usize>,
    last_baseline_index: Option<usize>,
}

impl ContextTracker {
    pub fn new() -> Self {
        Self {
            last_baseline_tokens: None,
            last_baseline_index: None,
        }
    }

    /// Update baseline from provider usage (call after each assistant message).
    ///
    /// Only input-side tokens count toward context size. Output tokens are
    /// excluded because the assistant message is already in `messages` and
    /// will be estimated via chars/4 for trailing-message accounting.
    pub fn record_usage(&mut self, usage: &Usage, message_index: usize) {
        let total = usage.input + usage.cache_read + usage.cache_write;
        if total > 0 {
            self.last_baseline_tokens = Some(total as usize);
            self.last_baseline_index = Some(message_index);
        }
    }

    /// Reset baseline after compaction.
    ///
    /// The provider baseline includes system prompt + tool definitions that
    /// compaction cannot reduce. Carrying it forward after message drops
    /// produces an inflated estimate (e.g. 176k for 13 messages). Resetting
    /// forces the next `estimate_context_tokens` to fall back to chars/4,
    /// and the next LLM call's `record_usage` will establish a fresh,
    /// accurate baseline.
    pub fn record_compaction(&mut self) {
        self.last_baseline_tokens = None;
        self.last_baseline_index = None;
    }

    /// Estimate current context size: baseline + chars/4 for trailing messages.
    ///
    /// `total_tokens(messages)` is used as a floor — when the provider baseline
    /// is stale (e.g. models that report `usage.input = 0`), the cheap chars/4
    /// estimate prevents the tracker from drifting to zero and skipping compaction.
    pub fn estimate_context_tokens(&self, messages: &[AgentMessage]) -> usize {
        let chars_floor = total_tokens(messages);
        match (self.last_baseline_tokens, self.last_baseline_index) {
            (Some(baseline_tokens), Some(idx)) if idx < messages.len() => {
                let trailing: usize = messages[idx + 1..].iter().map(message_tokens).sum();
                (baseline_tokens + trailing).max(chars_floor)
            }
            _ => chars_floor,
        }
    }

    /// Build a budget snapshot from the current tracker state and config.
    pub fn budget_snapshot(
        &self,
        messages: &[AgentMessage],
        ctx_config: Option<&ContextConfig>,
    ) -> ContextBudgetSnapshot {
        let estimated_tokens = self.estimate_context_tokens(messages);
        let (system_prompt_tokens, budget_tokens, context_window) = ctx_config
            .map(|c| {
                (
                    c.system_prompt_tokens,
                    c.max_context_tokens.saturating_sub(c.system_prompt_tokens),
                    c.max_context_tokens,
                )
            })
            .unwrap_or((0, 0, 0));
        ContextBudgetSnapshot {
            estimated_tokens,
            budget_tokens,
            system_prompt_tokens,
            context_window,
        }
    }

    /// Discard the baseline entirely.
    pub fn reset(&mut self) {
        self.last_baseline_tokens = None;
        self.last_baseline_index = None;
    }
}

impl Default for ContextTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Context budget snapshot
// ---------------------------------------------------------------------------

/// Point-in-time context budget snapshot, sourced from `ContextTracker`.
/// Shared by `LlmCallStart` and `ContextCompactionStart` events.
#[derive(Debug, Clone)]
pub struct ContextBudgetSnapshot {
    pub estimated_tokens: usize,
    pub budget_tokens: usize,
    pub system_prompt_tokens: usize,
    pub context_window: usize,
}

// ---------------------------------------------------------------------------
// Compaction budget state (runtime, passed into compact)
// ---------------------------------------------------------------------------

/// Runtime token state passed into compaction.
///
/// Separates dynamic state from static `ContextConfig`. The `estimated_tokens`
/// value comes from the most accurate source available — typically
/// `ContextTracker::estimate_context_tokens()` which uses real provider usage
/// data when available, falling back to chars/4 estimation.
#[derive(Debug, Clone)]
pub struct CompactionBudgetState {
    /// Current estimated context tokens.
    pub estimated_tokens: usize,
}

impl CompactionBudgetState {
    /// Build from a message list using pure chars/4 estimation.
    /// Useful in tests or when no provider usage data is available.
    pub fn from_messages(messages: &[AgentMessage]) -> Self {
        Self {
            estimated_tokens: total_tokens(messages),
        }
    }
}

// ---------------------------------------------------------------------------
// Context configuration
// ---------------------------------------------------------------------------

/// Configuration for context management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextConfig {
    /// Maximum context tokens (leave room for response)
    pub max_context_tokens: usize,
    /// Tokens reserved for the system prompt
    pub system_prompt_tokens: usize,
    /// Minimum recent messages to always keep (full detail)
    pub keep_recent: usize,
    /// Minimum first messages to always keep
    pub keep_first: usize,
    /// Max lines to keep per tool output in Level 1 compaction
    pub tool_output_max_lines: usize,
    /// Compaction trigger as percentage of budget (0–100).
    /// When context exceeds this fraction, L1 starts summarizing old turns.
    /// Set to 100 to disable early collapse. Default: 80.
    pub compact_trigger_pct: u8,
    /// Compaction target as percentage of budget (0–100).
    /// L1 and L2 both aim to reduce context to this fraction.
    /// Must be <= compact_trigger_pct. Default: 75.
    pub compact_target_pct: u8,
    /// Maximum messages before L1 summarization is forced, even if the
    /// token estimate is within budget. Guards against stale provider
    /// baselines that would otherwise skip compaction.
    /// Set to 0 to disable message-count trigger. Default: 80.
    pub max_messages: usize,
    /// Hard message-count limit that forces L2 eviction (dropping middle
    /// messages), even if the token estimate is within budget. This is the
    /// last-resort guard against sessions that accumulate thousands of
    /// non-compactable messages (e.g. steering prompts).
    /// Must be >= max_messages. Set to 0 to disable. Default: 250.
    pub max_messages_hard: usize,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_context_tokens: 100_000,
            system_prompt_tokens: 4_000,
            keep_recent: 10,
            keep_first: 2,
            tool_output_max_lines: 50,
            compact_trigger_pct: 80,
            compact_target_pct: 75,
            max_messages: 80,
            max_messages_hard: 250,
        }
    }
}

impl ContextConfig {
    /// Derive a context config from a model's context window size.
    ///
    /// Reserves 20% of the context window for output tokens, uses the rest
    /// as the compaction budget. All other settings use defaults.
    pub fn from_context_window(context_window: u32) -> Self {
        let max_context_tokens = (context_window as usize) * 80 / 100;
        Self {
            max_context_tokens,
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Execution limits
// ---------------------------------------------------------------------------

/// Execution limits for the agent loop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionLimits {
    /// Maximum number of turns (LLM calls)
    pub max_turns: usize,
    /// Maximum total tokens consumed
    pub max_total_tokens: usize,
    /// Maximum wall-clock time
    pub max_duration: std::time::Duration,
}

impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_turns: 50,
            max_total_tokens: 1_000_000,
            max_duration: std::time::Duration::from_secs(600),
        }
    }
}

/// Tracks execution state against limits
pub struct ExecutionTracker {
    pub limits: ExecutionLimits,
    pub turns: usize,
    pub tokens_used: usize,
    pub started_at: std::time::Instant,
}

impl ExecutionTracker {
    pub fn new(limits: ExecutionLimits) -> Self {
        Self {
            limits,
            turns: 0,
            tokens_used: 0,
            started_at: std::time::Instant::now(),
        }
    }

    pub fn record_turn(&mut self, tokens: usize) {
        self.turns += 1;
        self.tokens_used += tokens;
    }

    /// Check if any limit has been exceeded. Returns the reason if so.
    pub fn check_limits(&self) -> Option<String> {
        if self.turns >= self.limits.max_turns {
            return Some(format!(
                "Max turns reached ({}/{})",
                self.turns, self.limits.max_turns
            ));
        }
        if self.tokens_used >= self.limits.max_total_tokens {
            return Some(format!(
                "Max tokens reached ({}/{})",
                self.tokens_used, self.limits.max_total_tokens
            ));
        }
        let elapsed = self.started_at.elapsed();
        if elapsed >= self.limits.max_duration {
            return Some(format!(
                "Max duration reached ({:.0}s/{:.0}s)",
                elapsed.as_secs_f64(),
                self.limits.max_duration.as_secs_f64()
            ));
        }
        None
    }
}
