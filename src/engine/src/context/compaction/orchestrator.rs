//! Compaction orchestration — pressure-driven phase execution.
//!
//! Phases run in increasing lossiness:
//!   1. reclaim: recover content whose lifecycle has ended.
//!   2. shrink: locally reduce oversized/old content.
//!   3. collapse: summarize old assistant/tool turns under token pressure.
//!   4. evict: drop stale middle context under hard pressure.

use super::accounting::build_stats;
use super::accounting::collect_tool_details;
use super::accounting::image_count;
use super::accounting::StatsInput;
use super::phase::PhaseContext;
use super::phases::level0_reclaim::current_run;
use super::phases::level1_shrink;
use super::phases::level2_collapse::old_turns;
use super::phases::level3_evict::stale;
use super::policy::CompactionPolicy;
use super::sanitize::sanitize_tool_pairs;
use super::types::CompactionResult;
use crate::context::tokens::total_tokens;
use crate::context::tracking::CompactionBudgetState;
use crate::context::tracking::ContextConfig;
use crate::types::*;

/// Compaction phases, executed in order.
#[derive(Clone, Copy)]
enum Phase {
    /// Always-on lifecycle reclaim.
    Reclaim,
    /// Local truncation/clearing of individual messages and tool results.
    Shrink,
    /// Semantic summary of old turns.
    Collapse,
    /// Structural stale-context eviction.
    Evict,
}

impl Phase {
    fn level(self) -> u8 {
        match self {
            Phase::Reclaim => 0,
            Phase::Shrink => 1,
            Phase::Collapse => 2,
            Phase::Evict => 3,
        }
    }
}

const PHASES: [Phase; 4] = [Phase::Reclaim, Phase::Shrink, Phase::Collapse, Phase::Evict];

/// Compact messages using a pressure-driven pipeline.
pub fn compact_messages(
    messages: Vec<AgentMessage>,
    config: &ContextConfig,
    budget_state: &CompactionBudgetState,
) -> CompactionResult {
    let budget = config
        .max_context_tokens
        .saturating_sub(config.system_prompt_tokens);

    let compact_trigger = budget * (config.compact_trigger_pct.min(100) as usize) / 100;
    let compact_target_pct = config.compact_target_pct.min(config.compact_trigger_pct);
    let compact_target = budget * (compact_target_pct as usize) / 100;
    let message_tokens = total_tokens(&messages);

    let ctx = PhaseContext {
        budget,
        compact_trigger,
        compact_target,
        keep_recent: config.keep_recent,
        keep_first: config.keep_first,
        max_messages: config.max_messages,
        message_limit_target_pct: config.message_limit_target_pct,
        tool_output_max_lines: config.tool_output_max_lines,
        policy: CompactionPolicy::default(),
    };

    let before_message_count = messages.len();
    let before_image_count = image_count(&messages);
    let before_estimated_tokens = budget_state.estimated_tokens;
    let before_tool_details = collect_tool_details(&messages);

    let mut current_tokens = message_tokens;
    let mut messages = messages;
    let mut all_actions = Vec::new();
    let mut level = 0;

    let max_messages = config.max_messages;
    let over_message_limit = max_messages > 0 && messages.len() > max_messages;
    let should_collapse = message_tokens > ctx.compact_trigger && !over_message_limit;
    let should_evict = message_tokens > ctx.budget || over_message_limit;

    for phase in PHASES {
        let run_phase = match phase {
            Phase::Reclaim => true,
            Phase::Shrink => true,
            Phase::Collapse => should_collapse,
            Phase::Evict => should_evict,
        };

        if !run_phase {
            continue;
        }

        let pre_phase_tokens = total_tokens(&messages);
        let result = match phase {
            Phase::Reclaim => current_run::run(messages, &ctx),
            Phase::Shrink => level1_shrink::run(messages, &ctx, current_tokens),
            Phase::Collapse => old_turns::run(messages, &ctx, current_tokens),
            Phase::Evict => stale::run(messages, &ctx),
        };

        if !result.actions.is_empty() {
            level = level.max(phase.level());
        }

        let saved: usize = result
            .actions
            .iter()
            .map(|a| a.before_tokens.saturating_sub(a.after_tokens))
            .sum();
        current_tokens = current_tokens.saturating_sub(saved);

        if matches!(phase, Phase::Shrink) {
            let image_removed = before_image_count.saturating_sub(image_count(&result.messages));
            if image_removed > 0 {
                let provider_extra = before_estimated_tokens.saturating_sub(pre_phase_tokens);
                let image_provider_extra =
                    provider_extra / before_image_count.max(1) * image_removed;
                current_tokens = current_tokens.saturating_sub(image_provider_extra);
            }
        }

        current_tokens = current_tokens.max(total_tokens(&result.messages));
        all_actions.extend(result.actions);
        messages = result.messages;
    }

    let pre_sanitize_tokens = total_tokens(&messages);
    let messages = sanitize_tool_pairs(messages);
    let post_sanitize_tokens = total_tokens(&messages);
    let sanitize_removed = pre_sanitize_tokens.saturating_sub(post_sanitize_tokens);
    current_tokens = current_tokens.saturating_sub(sanitize_removed);
    current_tokens = current_tokens.max(post_sanitize_tokens);

    let after_message_count = messages.len();
    let after_message_tokens = current_tokens;
    let after_estimated_tokens = if after_message_tokens == message_tokens {
        before_estimated_tokens
    } else {
        before_estimated_tokens
            .saturating_sub(message_tokens.saturating_sub(after_message_tokens))
            .max(after_message_tokens)
    };
    let after_tool_details = collect_tool_details(&messages);

    let stats = build_stats(StatsInput {
        level,
        before_message_count,
        after_message_count,
        before_estimated_tokens,
        after_estimated_tokens,
        before_tool_details,
        after_tool_details,
        actions: all_actions,
    });

    CompactionResult { messages, stats }
}
