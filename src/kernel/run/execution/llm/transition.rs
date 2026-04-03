use std::time::Duration;

use super::assistant_turn::aborted_tool_result_messages;
use super::assistant_turn::record_assistant_turn;
use super::engine_state::RunLoopState;
use super::response_mapper::LLMResponse;
use crate::kernel::run::result::Reason;
use crate::llm::providers::common::is_context_overflow_message;
use crate::sessions::Message;
use crate::types::ErrorSource;

/// Maximum consecutive max_tokens continuations before accepting partial output.
const MAX_CONTINUATIONS: u32 = 5;

/// Maximum context-overflow compaction retries before giving up.
const MAX_OVERFLOW_RETRIES: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnTransition {
    Error(Reason),
    Abort(Reason),
    DispatchTools,
    /// Re-enter LLM without dispatching tools (e.g. max_tokens continuation).
    Continue,
    /// Context overflow detected — force compaction then retry the LLM call.
    CompactAndRetry,
    Done,
}

#[allow(clippy::too_many_arguments)]
pub fn apply_turn_result(
    messages: &mut Vec<Message>,
    state: &mut RunLoopState,
    turn: &LLMResponse,
    llm_error: Option<&str>,
    abort_reason: Option<Reason>,
    model: &str,
    max_duration: Duration,
    run_id: &str,
) -> TurnTransition {
    if let Some(err) = llm_error {
        // Detect context overflow — trigger compaction instead of failing.
        if is_context_overflow_message(err) {
            let retries = state.increment_overflow_retries();
            if retries <= MAX_OVERFLOW_RETRIES {
                return TurnTransition::CompactAndRetry;
            }
            // Exhausted retries — fall through to normal error handling.
        }

        messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "failed",
            serde_json::json!({"finish_reason": turn.finish_reason(), "error": err}),
        ));
        messages.push(Message::error(ErrorSource::Llm, err));
        state.record_error(err);
        return TurnTransition::Error(Reason::Error);
    }

    record_assistant_turn(messages, turn, state, model, max_duration, run_id);

    if turn.has_tool_calls() && state.should_continue() {
        if let Some(reason) = abort_reason {
            messages.extend(aborted_tool_result_messages(turn.tool_calls(), run_id));
            return TurnTransition::Abort(reason);
        }
        state.reset_max_tokens_streak();
        return TurnTransition::DispatchTools;
    }

    // Handle max_tokens truncation: ask LLM to continue.
    if turn.finish_reason() == "max_tokens" && !turn.has_tool_calls() {
        let streak = state.increment_max_tokens_streak();
        if streak < MAX_CONTINUATIONS {
            messages.push(Message::user("Continue from where you left off.").with_run_id(run_id));
            state.force_continue();
            return TurnTransition::Continue;
        }
        // Exceeded max continuations — accept partial response.
    } else {
        state.reset_max_tokens_streak();
    }

    TurnTransition::Done
}
