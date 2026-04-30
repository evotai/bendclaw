//! Context compaction: shrink context when approaching token budget.

use tokio::sync::mpsc;

use super::config::AgentLoopConfig;
use crate::context::CompactionBudgetState;
use crate::context::CompactionStrategy;
use crate::context::ContextTracker;
use crate::context::DefaultCompaction;
use crate::types::*;

/// Run context compaction if configured.
pub(super) fn compact_context(
    context: &mut AgentContext,
    config: &AgentLoopConfig,
    context_tracker: &mut ContextTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    let strategy: &dyn CompactionStrategy = config
        .compaction_strategy
        .as_deref()
        .unwrap_or(&DefaultCompaction);

    let ctx_config = match config.context_config {
        Some(ref c) => c,
        None => return,
    };

    let original_count = context.messages.len();
    let budget = context_tracker.budget_snapshot(&context.messages, Some(ctx_config));
    let pre_stats = crate::context::compute_call_stats_from_agent_messages(&context.messages);

    // Compact uses message-only estimate (chars/4), not the provider baseline.
    // Provider baseline includes system+tools overhead that compact can't reduce.
    let budget_state = CompactionBudgetState::from_messages(&context.messages);
    let compact_budget = crate::context::ContextBudgetSnapshot {
        estimated_tokens: budget_state.estimated_tokens,
        budget_tokens: budget.budget_tokens,
        system_prompt_tokens: budget.system_prompt_tokens,
        context_window: budget.context_window,
    };
    let result = strategy.compact(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    context.messages = result.messages;

    tx.send(AgentEvent::ContextCompactionStart {
        message_count: original_count,
        budget: compact_budget,
        message_stats: pre_stats,
    })
    .ok();

    // Drop any provider baseline after compaction runs, even if it skipped.
    // The baseline includes system+tools overhead that would otherwise pollute
    // the next LLM start snapshot and immediately make the context look over budget.
    context_tracker.record_compaction();

    tx.send(AgentEvent::ContextCompactionEnd {
        stats: result.stats,
        messages: context.messages.clone(),
        context_window: budget.context_window,
    })
    .ok();
}

/// Force compaction for error recovery.
///
/// Removes the last error message from both `context.messages` and
/// `new_messages`, then compacts. Returns true if recovery was performed.
/// All-or-nothing: if conditions aren't met, nothing is touched.
pub(super) fn compact_for_recovery(
    context: &mut AgentContext,
    new_messages: &mut Vec<AgentMessage>,
    config: &AgentLoopConfig,
    context_tracker: &mut ContextTracker,
    tx: &mpsc::UnboundedSender<AgentEvent>,
) -> bool {
    let strategy: &dyn CompactionStrategy = config
        .compaction_strategy
        .as_deref()
        .unwrap_or(&DefaultCompaction);

    let ctx_config = match config.context_config {
        Some(ref c) => c,
        None => return false,
    };

    let budget = context_tracker.budget_snapshot(&context.messages, Some(ctx_config));
    let message_tokens = crate::context::total_tokens(&context.messages);

    if message_tokens <= budget.budget_tokens / 2 {
        return false;
    }

    // Remove the error message before compaction so it doesn't pollute the result
    context.messages.pop();
    new_messages.pop();

    let pre_stats = crate::context::compute_call_stats_from_agent_messages(&context.messages);

    let budget_state = CompactionBudgetState::from_messages(&context.messages);
    let compact_result = strategy.compact(
        std::mem::take(&mut context.messages),
        ctx_config,
        &budget_state,
    );
    context.messages = compact_result.messages;
    context_tracker.record_compaction();

    tx.send(AgentEvent::ContextCompactionStart {
        message_count: compact_result.stats.before_message_count,
        budget: crate::context::ContextBudgetSnapshot {
            estimated_tokens: compact_result.stats.before_estimated_tokens,
            budget_tokens: budget.budget_tokens,
            system_prompt_tokens: budget.system_prompt_tokens,
            context_window: budget.context_window,
        },
        message_stats: pre_stats,
    })
    .ok();
    tx.send(AgentEvent::ContextCompactionEnd {
        stats: compact_result.stats,
        messages: context.messages.clone(),
        context_window: budget.context_window,
    })
    .ok();

    true
}
