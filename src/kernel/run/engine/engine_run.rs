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
use crate::observability::server_log;

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
        let loop_span = self
            .trace
            .start_span(
                "llm",
                "reasoning.loop",
                "",
                "",
                "{}",
                "llm reasoning started",
            )
            .await;
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
        server_log::info(
            &self.ops_ctx(iteration),
            "turn",
            "started",
            server_log::ServerFields::default()
                .detail("message_count", self.ctx.messages.len())
                .detail("max_context_tokens", state.max_context_tokens())
                .detail(
                    "tool_strategy",
                    format!("{:?}", self.ctx.tool_view.strategy()),
                ),
        );
        self.emit(Event::ReasonStart).await;

        let (turn, llm_error) = self.call_llm(state, iteration).await;

        self.emit(Event::ReasonEnd {
            finish_reason: turn.finish_reason().to_string(),
        })
        .await;

        if let Some(err) = llm_error {
            self.record_llm_error(&err, &turn);
            state.record_error(&err);
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
            server_log::error(
                &self.ops_ctx(iteration),
                "turn",
                "failed",
                server_log::ServerFields::default()
                    .tokens(turn.usage().total_tokens)
                    .bytes(turn.bytes())
                    .detail("finish_reason", turn.finish_reason())
                    .detail("tool_calls", turn.tool_calls().len())
                    .detail("error", err)
                    .detail("usage", turn.usage().clone())
                    .detail("chunk_count", turn.chunk_count()),
            );
            self.emit(Event::TurnEnd { iteration }).await;
            return Ok(StepOutcome::Error(Reason::Error));
        }

        self.record_assistant_message(&turn, state);

        if turn.has_tool_calls() && state.should_continue() {
            let abort = state.check_cancel_or_timeout(
                &self.abort_policy,
                self.cancel.is_cancelled(),
                Instant::now(),
            );
            if let Some(reason) = abort.reason {
                self.abort_tool_results(turn.tool_calls());
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
                server_log::warn(
                    &self.ops_ctx(iteration),
                    "turn",
                    "aborted",
                    server_log::ServerFields::default()
                        .detail("finish_reason", turn.finish_reason())
                        .detail("tool_calls", turn.tool_calls().len())
                        .detail("reason", reason.as_str()),
                );
                self.emit(Event::TurnEnd { iteration }).await;
                return Ok(StepOutcome::Abort(reason));
            }

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
            server_log::info(
                &self.ops_ctx(iteration),
                "turn",
                "completed",
                server_log::ServerFields::default()
                    .tokens(turn.usage().total_tokens)
                    .bytes(turn.bytes())
                    .detail("status", "tool_dispatch")
                    .detail("finish_reason", turn.finish_reason())
                    .detail("tool_calls", turn.tool_calls().len())
                    .detail("usage", turn.usage().clone())
                    .detail("chunk_count", turn.chunk_count()),
            );
            self.emit(Event::TurnEnd { iteration }).await;
            Ok(StepOutcome::Continue)
        } else {
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
            server_log::info(
                &self.ops_ctx(iteration),
                "turn",
                "completed",
                server_log::ServerFields::default()
                    .tokens(turn.usage().total_tokens)
                    .bytes(turn.bytes())
                    .detail("status", "done")
                    .detail("finish_reason", turn.finish_reason())
                    .detail("tool_calls", turn.tool_calls().len())
                    .detail("usage", turn.usage().clone())
                    .detail("chunk_count", turn.chunk_count())
                    .detail("content_blocks", turn.content_blocks()),
            );
            self.emit(Event::TurnEnd { iteration }).await;
            Ok(StepOutcome::Done)
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
        tracing::info!(
            duration_ms = self.start_time.elapsed().as_millis() as u64,
            iterations, prompt_tokens = usage.prompt_tokens,
            completion_tokens = usage.completion_tokens,
            ttft_ms = usage.ttft_ms,
            stop_reason = %stop_reason, "engine finished"
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
            AbortSignal::MaxIterations => tracing::warn!(
                iterations = state.iterations(),
                max = self.ctx.max_iterations,
                "max iterations reached"
            ),
            AbortSignal::Timeout => tracing::warn!(
                max_duration_secs = self.ctx.max_duration.as_secs(),
                "session timeout"
            ),
            _ => {}
        }
    }
}
