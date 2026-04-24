//! Compaction orchestration — types, stats, and level-driven execution.
//!
//! `compact_messages` runs compaction in levels:
//!   L0: always-on cleanup (clear expired, shrink oversized, image stripping)
//!   L1: summarize old turns, strip old images
//!   L2: drop middle messages
//!
//! L0 runs unconditionally. L1–L2 only run when over budget, and stop
//! as soon as the context fits within budget.

use serde::Deserialize;
use serde::Serialize;

use super::pass::CompactContext;
use super::passes::clear_expired;
use super::passes::collapse_old_turns;
use super::passes::evict_stale;
use super::passes::shrink_oversized;
use super::policy::CompactionPolicy;
use super::sanitize::sanitize_tool_pairs;
use crate::context::tokens::content_tokens;
use crate::context::tokens::total_tokens;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::*;

// ---------------------------------------------------------------------------
// Compaction types
// ---------------------------------------------------------------------------

/// Per-tool token breakdown entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolTokenDetail {
    pub tool_name: String,
    pub tokens: usize,
}

/// Describes what happened to a single item during compaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactionAction {
    /// Message index in the original list (0-based).
    pub index: usize,
    /// Tool name, "assistant", or "messages".
    pub tool_name: String,
    /// What method was used.
    pub method: CompactionMethod,
    /// Tokens before compaction.
    pub before_tokens: usize,
    /// Tokens after compaction.
    pub after_tokens: usize,
    /// End index for range actions (drop).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_index: Option<usize>,
    /// Count of related messages (e.g. tool results in a summarized turn).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_count: Option<usize>,
}

/// The method used to compact a message or tool result.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CompactionMethod {
    /// Tree-sitter structural outline extraction
    Outline,
    /// Head + tail truncation
    HeadTail,
    /// Turn summarized
    Summarized,
    /// Messages dropped
    Dropped,
    /// CurrentRun result cleared after use
    LifecycleCleared,
    /// Old result cleared by age policy
    #[serde(alias = "age_cleared")]
    AgeCleared,
    /// Oversized result capped
    #[serde(alias = "oversize_capped")]
    OversizeCapped,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompactionStats {
    /// Highest level that produced actions: 3=evict, 2=collapse, 1=shrink, 0=clear/no-op
    pub level: u8,
    pub before_message_count: usize,
    pub after_message_count: usize,
    pub before_estimated_tokens: usize,
    pub after_estimated_tokens: usize,
    pub tool_outputs_truncated: usize,
    pub turns_summarized: usize,
    pub messages_dropped: usize,
    pub current_run_cleared: usize,
    /// Count of oversized results capped.
    #[serde(default)]
    pub oversize_capped: usize,
    /// Count of old results cleared by age policy.
    #[serde(default)]
    pub age_cleared: usize,
    /// Per-tool token breakdown before compaction (sorted by tokens desc).
    #[serde(default)]
    pub before_tool_details: Vec<ToolTokenDetail>,
    /// Per-tool token breakdown after compaction (sorted by tokens desc).
    #[serde(default)]
    pub after_tool_details: Vec<ToolTokenDetail>,
    /// Per-message compaction actions.
    #[serde(default)]
    pub actions: Vec<CompactionAction>,
}

#[derive(Debug, Clone)]
pub struct CompactionResult {
    pub messages: Vec<AgentMessage>,
    pub stats: CompactionStats,
}

pub trait CompactionStrategy: Send + Sync {
    fn compact(
        &self,
        messages: Vec<AgentMessage>,
        config: &ContextConfig,
        budget_state: &CompactionBudgetState,
    ) -> CompactionResult;
}

pub struct DefaultCompaction;

impl CompactionStrategy for DefaultCompaction {
    fn compact(
        &self,
        messages: Vec<AgentMessage>,
        config: &ContextConfig,
        budget_state: &CompactionBudgetState,
    ) -> CompactionResult {
        compact_messages(messages, config, budget_state)
    }
}

// ---------------------------------------------------------------------------
// Compaction levels
// ---------------------------------------------------------------------------

/// Compaction levels, executed in order.
#[derive(Clone, Copy)]
enum Level {
    /// Always-on: clear expired results, shrink oversized, strip old images
    L0Cleanup,
    /// Budget-gated: summarize old assistant turns
    L1Collapse,
    /// Budget-gated: drop middle messages
    L2Evict,
}

const LEVELS: [Level; 3] = [Level::L0Cleanup, Level::L1Collapse, Level::L2Evict];

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Collect per-tool token details from messages, sorted by tokens descending.
fn collect_tool_details(messages: &[AgentMessage]) -> Vec<ToolTokenDetail> {
    let mut details = Vec::new();
    for msg in messages {
        if let AgentMessage::Llm(Message::ToolResult {
            tool_name, content, ..
        }) = msg
        {
            details.push(ToolTokenDetail {
                tool_name: tool_name.clone(),
                tokens: content_tokens(content),
            });
        }
    }
    details.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    details
}

// ---------------------------------------------------------------------------
// Core
// ---------------------------------------------------------------------------

/// Compact messages using level-driven execution.
///
/// L0 runs unconditionally. L1–L2 only run when over budget, stopping
/// as soon as the context fits.
///
/// `budget_state.estimated_tokens` provides the initial token estimate from
/// the most accurate source available (e.g. provider usage data). This value
/// drives all budget gate and stop decisions. After each pass, it is updated
/// via action deltas and calibrated against `total_tokens()` as a floor.
pub fn compact_messages(
    messages: Vec<AgentMessage>,
    config: &ContextConfig,
    budget_state: &CompactionBudgetState,
) -> CompactionResult {
    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);

    // Trigger: L1 starts summarizing when context exceeds this.
    let compact_trigger = budget * (config.compact_trigger_pct.min(100) as usize) / 100;

    // Target: L1 and L2 both aim to reduce context to this fraction of budget.
    let compact_target_pct = config.compact_target_pct.min(config.compact_trigger_pct);
    let compact_target = budget * (compact_target_pct as usize) / 100;

    let ctx = CompactContext {
        budget,
        compact_trigger,
        compact_target,
        keep_recent: config.keep_recent,
        keep_first: config.keep_first,
        tool_output_max_lines: config.tool_output_max_lines,
        policy: CompactionPolicy::default(),
    };

    let before_message_count = messages.len();
    let before_estimated_tokens = budget_state.estimated_tokens;
    let before_tool_details = collect_tool_details(&messages);

    let mut current_tokens = budget_state.estimated_tokens;
    let mut messages = messages;
    let mut all_actions = Vec::new();

    for level in LEVELS {
        // L0 always runs.
        // L1 triggers at compact_trigger (92% of budget) — early collapse.
        // L2 triggers at budget (hard limit) — last resort eviction.
        let threshold = match level {
            Level::L0Cleanup => 0,
            Level::L1Collapse => ctx.compact_trigger,
            Level::L2Evict => ctx.budget,
        };
        if !matches!(level, Level::L0Cleanup) && current_tokens <= threshold {
            continue;
        }

        let result = match level {
            Level::L0Cleanup => {
                let r1 = clear_expired::run(messages, &ctx);
                // Update current_tokens between L0 sub-passes so shrink_oversized
                // sees the effect of clear_expired (avoids over-compaction).
                let r1_saved: usize = r1
                    .actions
                    .iter()
                    .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
                    .sum();
                let tokens_after_r1 = current_tokens.saturating_sub(r1_saved);
                let tokens_after_r1 = tokens_after_r1.max(total_tokens(&r1.messages));

                let r2 = shrink_oversized::run(r1.messages, &ctx, tokens_after_r1);
                let mut actions = r1.actions;
                actions.extend(r2.actions);
                super::pass::PassResult {
                    messages: r2.messages,
                    actions,
                }
            }
            Level::L1Collapse => collapse_old_turns::run(messages, &ctx, current_tokens),
            Level::L2Evict => evict_stale::run(messages, &ctx),
        };

        // Update current_tokens: delta from actions + floor calibration
        let saved: usize = result
            .actions
            .iter()
            .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
            .sum();
        current_tokens = current_tokens.saturating_sub(saved);
        current_tokens = current_tokens.max(total_tokens(&result.messages));

        all_actions.extend(result.actions);
        messages = result.messages;
    }

    let pre_sanitize_tokens = total_tokens(&messages);
    let messages = sanitize_tool_pairs(messages);
    let post_sanitize_tokens = total_tokens(&messages);
    // Sanitize may remove orphan messages without producing actions.
    // Reflect the removal by subtracting the delta, then floor-calibrate.
    let sanitize_removed = pre_sanitize_tokens.saturating_sub(post_sanitize_tokens);
    current_tokens = current_tokens.saturating_sub(sanitize_removed);
    current_tokens = current_tokens.max(post_sanitize_tokens);

    let after_message_count = messages.len();
    // after_estimated_tokens is the unified budget estimate after compaction,
    // not a provider-measured value. It reflects action deltas calibrated
    // against chars/4 as a floor.
    let after_estimated_tokens = current_tokens;
    let after_tool_details = collect_tool_details(&messages);

    // Derive counters from actions
    let mut current_run_cleared: usize = 0;
    let mut age_cleared: usize = 0;
    let mut oversize_capped: usize = 0;
    let mut tool_outputs_truncated: usize = 0;
    let mut turns_summarized: usize = 0;
    let mut messages_dropped: usize = 0;

    for action in &all_actions {
        match action.method {
            CompactionMethod::LifecycleCleared => current_run_cleared += 1,
            CompactionMethod::AgeCleared => age_cleared += 1,
            CompactionMethod::OversizeCapped => oversize_capped += 1,
            CompactionMethod::Outline | CompactionMethod::HeadTail => tool_outputs_truncated += 1,
            CompactionMethod::Summarized => turns_summarized += 1,
            CompactionMethod::Dropped => {
                messages_dropped += action.related_count.unwrap_or(1);
            }
        }
    }

    // level = highest action severity (matches CLI expectations)
    // 0=no-op, 1=shrink/cleanup, 2=collapse/summarize, 3=evict/drop
    let level = if messages_dropped > 0 {
        3
    } else if turns_summarized > 0 {
        2
    } else if tool_outputs_truncated > 0 || oversize_capped > 0 || age_cleared > 0 {
        1
    } else {
        0
    };

    CompactionResult {
        messages,
        stats: CompactionStats {
            level,
            before_message_count,
            after_message_count,
            before_estimated_tokens,
            after_estimated_tokens,
            tool_outputs_truncated,
            turns_summarized,
            messages_dropped,
            current_run_cleared,
            oversize_capped,
            age_cleared,
            before_tool_details,
            after_tool_details,
            actions: all_actions,
        },
    }
}

// ---------------------------------------------------------------------------
// Legacy re-exports for backward compatibility
// ---------------------------------------------------------------------------

/// Re-export `truncate_text_head_tail` from the shrink pass for external use.
pub use super::passes::shrink_oversized::truncate_text_head_tail;
