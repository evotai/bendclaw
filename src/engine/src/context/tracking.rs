//! Context tracking, configuration, and execution limits.

use serde::Deserialize;
use serde::Serialize;

use super::tokens::message_tokens;
use super::tokens::tool_definition_tokens;
use super::tokens::total_tokens;
use crate::provider::ToolDefinition;
use crate::types::*;

// ---------------------------------------------------------------------------
// Context tracking (real usage + estimates)
// ---------------------------------------------------------------------------

/// Tracks context size using provider usage as baseline, with chars/4
/// fallback for LLM call UI snapshots.
///
/// Important: the provider baseline includes system prompt + tool definitions
/// (fixed overhead) that compaction cannot reduce. Compaction uses
/// `total_tokens(messages)` directly instead of the tracker estimate.
pub struct ContextTracker {
    last_baseline_tokens: Option<usize>,
    last_baseline_index: Option<usize>,
    system_tool_overhead_tokens: usize,
}

impl ContextTracker {
    pub fn new() -> Self {
        Self {
            last_baseline_tokens: None,
            last_baseline_index: None,
            system_tool_overhead_tokens: 0,
        }
    }

    /// Record fixed request overhead that compaction cannot reduce.
    pub fn record_request_overhead(&mut self, system_prompt: &str, tools: &[ToolDefinition]) {
        self.system_tool_overhead_tokens =
            crate::context::estimate_tokens(system_prompt) + tool_definition_tokens(tools);
    }

    pub fn system_tool_overhead_tokens(&self) -> usize {
        self.system_tool_overhead_tokens
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

    /// Adjust baseline after compaction by subtracting saved tokens.
    ///
    /// Resetting the baseline entirely would cause fallback to chars/4, which
    /// severely underestimates images. Instead, subtract what compaction saved
    /// so the baseline still reflects the real API cost of remaining content
    /// (especially images that compaction cannot reduce).
    pub fn record_compaction_savings(&mut self, tokens_saved: usize) {
        if let Some(ref mut baseline) = self.last_baseline_tokens {
            *baseline = baseline.saturating_sub(tokens_saved);
        }
    }

    /// Reset baseline entirely. Use only when messages are replaced wholesale
    /// (e.g., full conversation summary replacing all prior messages).
    pub fn record_compaction(&mut self) {
        self.last_baseline_tokens = None;
        self.last_baseline_index = None;
        self.system_tool_overhead_tokens = 0;
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
            tool_definition_tokens: self
                .system_tool_overhead_tokens
                .saturating_sub(system_prompt_tokens),
            context_window,
        }
    }

    /// Discard the baseline entirely.
    pub fn reset(&mut self) {
        self.last_baseline_tokens = None;
        self.last_baseline_index = None;
        self.system_tool_overhead_tokens = 0;
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
    pub tool_definition_tokens: usize,
    pub context_window: usize,
}

// ---------------------------------------------------------------------------
// Compaction budget state (runtime, passed into compact)
// ---------------------------------------------------------------------------

/// Runtime token state passed into compaction.
///
/// Uses message-only token estimates (chars/4) so compaction decisions
/// are based on what it can actually reduce — message content — rather
/// than the full provider input which includes system prompt + tool
/// definitions that compaction cannot touch.
#[derive(Debug, Clone)]
pub struct CompactionBudgetState {
    /// Current estimated message tokens (chars/4, excludes system/tools overhead).
    pub estimated_tokens: usize,
}

impl CompactionBudgetState {
    /// Build from a message list using pure chars/4 estimation.
    pub fn from_messages(messages: &[AgentMessage]) -> Self {
        Self {
            estimated_tokens: total_tokens(messages),
        }
    }

    /// Provider baseline should not force compaction decisions: usage can include
    /// prompt-cache reads that make context appear much larger than compactable
    /// message content. LLM-call UI still uses the tracker baseline separately.
    pub fn from_tracker(_tracker: &ContextTracker, messages: &[AgentMessage]) -> Self {
        Self::from_messages(messages)
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
    /// Maximum messages before L2 eviction drops stale middle context, even if
    /// the token estimate is within budget. This keeps long sessions compact
    /// and avoids accumulating low-value summaries forever.
    /// Set to 0 to disable message-count eviction. Default: 80.
    pub max_messages: usize,
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
