use std::time::Instant;

use tokio_stream::StreamExt;

use super::diagnostics;
use super::engine::Engine;
use crate::kernel::run::event::Delta;
use crate::kernel::run::event::Event;
use crate::kernel::run::run_loop::LLMResponse;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::trace::SpanMeta;
use crate::kernel::Message;
use crate::kernel::OpType;
use crate::kernel::OperationMeta;

impl Engine {
    pub(super) async fn call_llm(
        &mut self,
        state: &mut RunLoopState,
        iteration: u32,
    ) -> (LLMResponse, Option<String>) {
        let prepared = self.prepare_llm_request(iteration);
        let prepared_summary = diagnostics::summarize_prepared_llm_request(&prepared);
        diagnostics::log_llm_context(self.ops_ctx(iteration), &prepared_summary);
        diagnostics::log_llm_request(
            self.ops_ctx(iteration),
            self.ctx.model.as_ref(),
            &format!("{:?}", self.ctx.tool_view.strategy()),
            self.ctx.temperature,
            prepared.request_bytes,
            &prepared_summary,
        );
        self.emit_audit("llm.request", prepared.request_payload.clone())
            .await;

        let _reasoning_tracker =
            OperationMeta::begin(OpType::Reasoning).timeout(self.ctx.max_duration);
        let stream = self.ctx.llm.chat_stream(
            &self.ctx.model,
            &prepared.chat_messages,
            &prepared.active_tools,
            self.ctx.temperature,
        );
        let llm_span = self.trace.start_span(
            "llm",
            "reasoning.turn",
            &self.loop_span_id,
            "reasoning",
            &SpanMeta::LlmTurn { iteration }.to_json(),
            "llm reasoning turn started",
        );

        self.ctx.messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "started",
            serde_json::json!({"iteration": iteration}),
        ));

        let mut turn = self.collect_response(stream).await;
        let ttft_ms = turn.ttft_ms().unwrap_or(0);
        state.merge_usage(turn.usage());

        if ttft_ms > 0 && state.usage().ttft_ms == 0 {
            state.set_ttft(ttft_ms);
        }

        let llm_error = if turn.has_error() {
            let err = turn.take_error().unwrap_or_default();
            self.handle_llm_error(iteration, &llm_span, &turn, &err, ttft_ms)
                .await;
            Some(err)
        } else {
            self.handle_llm_success(iteration, &llm_span, &turn, ttft_ms)
                .await;
            None
        };

        (turn, llm_error)
    }

    async fn handle_llm_error(
        &mut self,
        iteration: u32,
        llm_span: &crate::kernel::trace::TraceSpan,
        turn: &LLMResponse,
        err: &str,
        ttft_ms: u64,
    ) {
        let ms = llm_span.elapsed_ms();
        llm_span
            .fail(
                ms,
                "llm_stream_error",
                err,
                &SpanMeta::LlmFailed {
                    finish_reason: turn.finish_reason().to_string(),
                    error: err.to_string(),
                }
                .to_json(),
                "llm reasoning failed",
            )
            .await;
        self.emit(Event::ReasonError {
            error: err.to_string(),
        })
        .await;

        let mut payload = diagnostics::build_llm_response_payload(
            self.audit_payload(iteration),
            self.ctx.model.as_ref(),
            turn,
            ttft_ms,
        );
        payload.insert("error".to_string(), serde_json::json!(err));
        diagnostics::log_llm_failure(
            self.ops_ctx(iteration),
            self.ctx.model.as_ref(),
            turn,
            err,
            ms,
            ttft_ms,
        );
        self.emit_audit("llm.error", payload).await;
    }

    async fn handle_llm_success(
        &mut self,
        iteration: u32,
        llm_span: &crate::kernel::trace::TraceSpan,
        turn: &LLMResponse,
        ttft_ms: u64,
    ) {
        let ms = llm_span.elapsed_ms();
        llm_span
            .complete(
                ms,
                ttft_ms,
                turn.usage().prompt_tokens,
                turn.usage().completion_tokens,
                0,
                0.0,
                &SpanMeta::LlmCompleted {
                    finish_reason: turn.finish_reason().to_string(),
                    prompt_tokens: turn.usage().prompt_tokens,
                    completion_tokens: turn.usage().completion_tokens,
                }
                .to_json(),
                "llm reasoning turn completed",
            )
            .await;
        self.ctx.messages.push(Message::operation_event(
            "llm",
            "reasoning.turn",
            "completed",
            serde_json::json!({
                "finish_reason": turn.finish_reason(),
                "prompt_tokens": turn.usage().prompt_tokens,
                "completion_tokens": turn.usage().completion_tokens,
            }),
        ));

        diagnostics::log_llm_success(
            self.ops_ctx(iteration),
            self.ctx.model.as_ref(),
            turn,
            ms,
            ttft_ms,
        );
        diagnostics::log_llm_final_output(self.ops_ctx(iteration), turn);
        self.emit_audit(
            "llm.response",
            diagnostics::build_llm_response_payload(
                self.audit_payload(iteration),
                self.ctx.model.as_ref(),
                turn,
                ttft_ms,
            ),
        )
        .await;
    }

    async fn collect_response(&self, stream: crate::llm::stream::ResponseStream) -> LLMResponse {
        let mut resp = LLMResponse::new();
        let mut stream = stream;
        let mut first_token_seen = false;
        let stream_start = Instant::now();
        let mut chunk_count = 0u32;
        let mut bytes = 0u64;
        loop {
            tokio::select! {
                event = stream.next() => {
                    match event {
                        Some(event) => {
                            chunk_count += 1;
                            bytes += match &event {
                                crate::llm::stream::StreamEvent::ContentDelta(chunk)
                                | crate::llm::stream::StreamEvent::ThinkingDelta(chunk)
                                | crate::llm::stream::StreamEvent::Error(chunk) => chunk.len() as u64,
                                crate::llm::stream::StreamEvent::ToolCallStart { id, name, .. } => {
                                    (id.len() + name.len()) as u64
                                }
                                crate::llm::stream::StreamEvent::ToolCallDelta { json_chunk, .. } => {
                                    json_chunk.len() as u64
                                }
                                crate::llm::stream::StreamEvent::ToolCallEnd { id, name, arguments, .. } => {
                                    (id.len() + name.len() + arguments.len()) as u64
                                }
                                crate::llm::stream::StreamEvent::Usage(_) => 0,
                                crate::llm::stream::StreamEvent::Done { finish_reason, provider, model } => {
                                    (finish_reason.len()
                                        + provider.as_ref().map_or(0, |s| s.len())
                                        + model.as_ref().map_or(0, |s| s.len())) as u64
                                }
                            };
                            if !first_token_seen {
                                if let Some(delta) = Delta::from_stream_event(&event) {
                                    if matches!(&delta, Delta::Text { .. } | Delta::Thinking { .. }) {
                                        resp.set_ttft_ms(stream_start.elapsed().as_millis() as u64);
                                        first_token_seen = true;
                                    }
                                    self.emit(Event::StreamDelta(delta)).await;
                                }
                            } else if let Some(delta) = Delta::from_stream_event(&event) {
                                self.emit(Event::StreamDelta(delta)).await;
                            }
                            resp.apply_stream_event(event);
                        }
                        None => break,
                    }
                }
                _ = self.cancel.cancelled() => {
                    resp.mark_cancelled();
                    break;
                }
            }
        }
        resp.set_stream_stats(chunk_count, bytes);
        resp
    }
}
