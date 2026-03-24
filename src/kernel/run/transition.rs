use std::time::Duration;

use crate::base::ErrorSource;
use crate::kernel::run::orchestration::aborted_tool_result_messages;
use crate::kernel::run::orchestration::record_assistant_turn;
use crate::kernel::run::result::Reason;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::Message;

/// Maximum consecutive max_tokens continuations before accepting partial output.
const MAX_CONTINUATIONS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnTransition {
    Error(Reason),
    Abort(Reason),
    DispatchTools,
    /// Re-enter LLM without dispatching tools (e.g. max_tokens continuation).
    Continue,
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
