use std::sync::atomic::Ordering;
use std::time::Instant;

use super::engine::Engine;
use crate::base::Result;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::ContentBlock;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::run::result::Usage;
use crate::kernel::run::run_loop::AbortSignal;
use crate::kernel::run::run_loop::RunLoopConfig;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::run::transition::apply_turn_result;
use crate::kernel::run::transition::TurnTransition;
use crate::observability::log::run_log;
use crate::observability::log::slog;

pub(super) enum StepOutcome {
    Continue,
    Done,
    Abort(Reason),
    Error(Reason),
}

impl Engine {
    pub async fn run(&mut self) -> Result<AgentResult> {
        let mut state = RunLoopState::new(RunLoopConfig::from_context(&self.ctx), Instant::now());

        self.emit(Event::Start).await;
        let loop_span = self.trace.start_span(
            "llm",
            "reasoning.loop",
            "",
            "",
            "{}",
            "llm reasoning started",
        );
        self.loop_span_id = loop_span.span_id.clone();

        while state.should_continue() {
            if let Some(reason) = self.check_abort(&state) {
                return self.abort(state, reason).await;
            }
            self.try_compact(&mut state).await;
            match self.step(&mut state).await? {
                StepOutcome::Continue => {}
                StepOutcome::Done => break,
                StepOutcome::Abort(reason) => return self.abort(state, reason).await,
                StepOutcome::Error(reason) => {
                    return self
                        .finish(
                            state.final_content().to_vec(),
                            state.iterations(),
                            state.usage().clone(),
                            reason,
                        )
                        .await;
                }
            }
        }

        let (content, iterations, usage) = state.into_finish();
        self.finish(content, iterations, usage, Reason::EndTurn)
            .await
    }

    async fn step(&mut self, state: &mut RunLoopState) -> Result<StepOutcome> {
        let iteration = state.begin_iteration();
        self.iteration.store(iteration, Ordering::Relaxed);
        self.emit(Event::TurnStart { iteration }).await;
        let payload = self.audit_payload(iteration);
        self.emit_audit("turn.started", payload).await;
        run_log!(info, self.ops_ctx(iteration), "turn", "started",
            msg = format!("  iter-{iteration}"),
            tool_strategy = %format!("{:?}", self.ctx.tool_view.strategy()),
            max_context_tokens = state.max_context_tokens(),
            message_count = self.ctx.messages.len(),
        );

        self.emit(Event::ReasonStart).await;

        let (turn, llm_error) = self.call_llm(state, iteration).await;

        self.emit(Event::ReasonEnd {
            finish_reason: turn.finish_reason().to_string(),
        })
        .await;

        let abort_reason = if turn.has_tool_calls() && state.should_continue() {
            state
                .check_cancel_or_timeout(
                    &self.abort_policy,
                    self.cancel.is_cancelled(),
                    Instant::now(),
                )
                .reason
        } else {
            None
        };

        match apply_turn_result(
            &mut self.ctx.messages,
            state,
            &turn,
            llm_error.as_deref(),
            abort_reason,
            self.ctx.model.as_ref(),
            self.ctx.max_duration,
        ) {
            TurnTransition::Error(reason) => {
                let err = llm_error.unwrap_or_default();
                let mut payload = self.audit_payload(iteration);
                payload.insert("status".to_string(), serde_json::json!("failed"));
                payload.insert(
                    "finish_reason".to_string(),
                    serde_json::json!(turn.finish_reason()),
                );
                payload.insert(
                    "tool_calls".to_string(),
                    serde_json::json!(turn.tool_calls().len() as u64),
                );
                self.emit_audit("turn.completed", payload).await;
                run_log!(error, self.ops_ctx(iteration), "turn", "failed",
                    msg = format!("  iter-{iteration} FAILED"),
                    finish_reason = %turn.finish_reason(),
                    error = %err,
                    tool_calls = turn.tool_calls().len(),
                    tokens = turn.usage().total_tokens,
                    bytes = turn.bytes(),
                    chunk_count = turn.chunk_count(),
                );

                self.emit(Event::TurnEnd { iteration }).await;
                Ok(StepOutcome::Error(reason))
            }
            TurnTransition::Abort(reason) => {
                let mut payload = self.audit_payload(iteration);
                payload.insert("status".to_string(), serde_json::json!("aborted"));
                payload.insert(
                    "finish_reason".to_string(),
                    serde_json::json!(turn.finish_reason()),
                );
                payload.insert(
                    "tool_calls".to_string(),
                    serde_json::json!(turn.tool_calls().len() as u64),
                );
                payload.insert("reason".to_string(), serde_json::json!(reason.as_str()));
                self.emit_audit("turn.completed", payload).await;
                run_log!(warn, self.ops_ctx(iteration), "turn", "aborted",
                    msg = format!("  iter-{iteration} aborted"),
                    reason = %reason.as_str(),
                    finish_reason = %turn.finish_reason(),
                    tool_calls = turn.tool_calls().len(),
                );
                self.emit(Event::TurnEnd { iteration }).await;
                Ok(StepOutcome::Abort(reason))
            }
            TurnTransition::DispatchTools => {
                self.dispatch_tools(turn.tool_calls(), state).await;
                let mut payload = self.audit_payload(iteration);
                payload.insert("status".to_string(), serde_json::json!("tool_dispatch"));
                payload.insert(
                    "finish_reason".to_string(),
                    serde_json::json!(turn.finish_reason()),
                );
                payload.insert(
                    "tool_calls".to_string(),
                    serde_json::json!(turn.tool_calls().len() as u64),
                );
                self.emit_audit("turn.completed", payload).await;
                run_log!(info, self.ops_ctx(iteration), "turn", "tool_dispatch",
                    msg = format!("  iter-{iteration} dispatched"),
                    finish_reason = %turn.finish_reason(),
                    tool_calls = turn.tool_calls().len(),
                    tokens = turn.usage().total_tokens,
                    bytes = turn.bytes(),
                    chunk_count = turn.chunk_count(),
                );

                self.emit(Event::TurnEnd { iteration }).await;
                Ok(StepOutcome::Continue)
            }
            TurnTransition::Done => {
                let mut payload = self.audit_payload(iteration);
                payload.insert("status".to_string(), serde_json::json!("done"));
                payload.insert(
                    "finish_reason".to_string(),
                    serde_json::json!(turn.finish_reason()),
                );
                payload.insert(
                    "tool_calls".to_string(),
                    serde_json::json!(turn.tool_calls().len() as u64),
                );
                self.emit_audit("turn.completed", payload).await;
                run_log!(info, self.ops_ctx(iteration), "turn", "done",
                    msg = format!("  iter-{iteration} done"),
                    finish_reason = %turn.finish_reason(),
                    tool_calls = turn.tool_calls().len(),
                    tokens = turn.usage().total_tokens,
                    bytes = turn.bytes(),
                    chunk_count = turn.chunk_count(),
                );

                self.emit(Event::TurnEnd { iteration }).await;
                Ok(StepOutcome::Done)
            }
        }
    }

    fn check_abort(&self, state: &RunLoopState) -> Option<Reason> {
        let abort = state.check_abort(
            &self.abort_policy,
            self.cancel.is_cancelled(),
            Instant::now(),
        );
        if let Some(reason) = &abort.reason {
            self.log_abort(abort.signal, state);
            Some(reason.clone())
        } else {
            None
        }
    }

    async fn abort(&mut self, state: RunLoopState, reason: Reason) -> Result<AgentResult> {
        self.emit(Event::Aborted {
            reason: reason.clone(),
        })
        .await;
        self.finish(
            state.final_content().to_vec(),
            state.iterations(),
            state.usage().clone(),
            reason,
        )
        .await
    }
    async fn finish(
        &self,
        content: Vec<ContentBlock>,
        iterations: u32,
        usage: Usage,
        stop_reason: Reason,
    ) -> Result<AgentResult> {
        self.emit(Event::End {
            iterations,
            stop_reason: stop_reason.as_str().to_string(),
            usage: usage.clone(),
        })
        .await;
        let dur = self.start_time.elapsed().as_millis() as u64;
        slog!(debug, "run", "finished",
            elapsed_ms = dur,
            iterations,
            prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            ttft_ms = usage.ttft_ms,
            stop_reason = %stop_reason,
        );
        Ok(AgentResult {
            content,
            iterations,
            usage,
            stop_reason,
            messages: self.ctx.messages.clone(),
        })
    }

    fn log_abort(&self, signal: AbortSignal, state: &RunLoopState) {
        match signal {
            AbortSignal::MaxIterations => slog!(
                warn,
                "run",
                "aborted",
                reason = "max_iterations",
                iterations = state.iterations(),
                max = self.ctx.max_iterations,
            ),
            AbortSignal::Timeout => slog!(
                warn,
                "run",
                "aborted",
                reason = "timeout",
                max_duration_secs = self.ctx.max_duration.as_secs(),
            ),
            _ => {}
        }
    }
}
