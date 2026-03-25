use std::sync::atomic::Ordering;
use std::time::Instant;

use super::diagnostics;
use super::engine::Engine;
use crate::base::Result;
use crate::kernel::run::event::Event;
use crate::kernel::run::result::ContentBlock;
use crate::kernel::run::result::Reason;
use crate::kernel::run::result::Result as AgentResult;
use crate::kernel::run::result::Usage;
use crate::kernel::run::run_loop::AbortSignal;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopConfig;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::run::transition::apply_turn_result;
use crate::kernel::run::transition::TurnTransition;

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
            self.drain_inbox().await;
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
        self.emit_audit("turn.started", self.audit_payload(iteration))
            .await;
        diagnostics::log_turn_started(
            self.ops_ctx(iteration),
            iteration,
            &format!("{:?}", self.ctx.tool_view.strategy()),
            state,
            self.ctx.messages.len(),
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
            self.ctx.run_id.as_ref(),
        ) {
            TurnTransition::Error(reason) => {
                let err = llm_error.unwrap_or_default();
                self.emit_turn_end(iteration, "failed", &turn, &[(
                    "error",
                    serde_json::json!(err),
                )])
                .await;
                Ok(StepOutcome::Error(reason))
            }
            TurnTransition::Abort(reason) => {
                self.emit_turn_end(iteration, "aborted", &turn, &[(
                    "reason",
                    serde_json::json!(reason.as_str()),
                )])
                .await;
                Ok(StepOutcome::Abort(reason))
            }
            TurnTransition::DispatchTools => {
                self.dispatch_tools(turn.tool_calls(), state).await;
                self.emit_turn_end(iteration, "tool_dispatch", &turn, &[])
                    .await;
                Ok(StepOutcome::Continue)
            }
            TurnTransition::Continue => {
                self.emit_turn_end(iteration, "continue", &turn, &[]).await;
                Ok(StepOutcome::Continue)
            }
            TurnTransition::Done => {
                self.emit_turn_end(iteration, "done", &turn, &[]).await;
                Ok(StepOutcome::Done)
            }
        }
    }

    async fn emit_turn_end(
        &self,
        iteration: u32,
        status: &str,
        turn: &LLMResponse,
        extra: &[(&str, serde_json::Value)],
    ) {
        let payload = diagnostics::build_turn_completed_payload(
            self.audit_payload(iteration),
            status,
            turn,
            extra,
        );
        self.emit_audit("turn.completed", payload).await;
        diagnostics::log_turn_completed(self.ops_ctx(iteration), iteration, status, turn);

        self.emit(Event::TurnEnd { iteration }).await;
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
        Ok(AgentResult {
            content,
            iterations,
            usage,
            stop_reason,
            checkpoint: self.latest_checkpoint.clone(),
            messages: self.ctx.messages.clone(),
        })
    }

    fn log_abort(&self, signal: AbortSignal, state: &RunLoopState) {
        diagnostics::log_abort_signal(
            signal,
            state.iterations(),
            self.ctx.max_iterations,
            self.ctx.max_duration.as_secs(),
        );
    }

    async fn drain_inbox(&mut self) {
        while let Ok(msg) = self.inbox.try_recv() {
            diagnostics::log_message_injected(&self.ctx.session_id);
            self.emit(Event::MessageInjected {
                content: msg.text(),
            })
            .await;
            self.ctx
                .messages
                .push(msg.with_run_id(self.ctx.run_id.to_string()));
        }
    }
}
