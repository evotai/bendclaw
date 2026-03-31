use std::collections::HashMap;
use std::sync::atomic::Ordering;

use super::diagnostics;
use super::engine::Engine;
use crate::kernel::execution::DispatchOutcome;
use crate::kernel::execution::ParsedToolCall;
use crate::kernel::execution::ToolCallResult;
use crate::kernel::memory::pressure;
use crate::kernel::memory::pressure::PressureLevel;
use crate::kernel::run::event::Event;
use crate::kernel::run::hooks::SteeringDecision;
use crate::kernel::run::prompt_projection;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::trace::SpanMeta;
use crate::kernel::trace::TraceSpan;
use crate::kernel::Message;
use crate::kernel::OperationMeta;
use crate::llm::message::ToolCall;
use crate::observability::server_log;

/// Max bytes for span error messages stored in trace DB.
const MAX_SPAN_ERROR: usize = 2_000;

impl Engine {
    pub(super) async fn dispatch_tools(
        &mut self,
        tool_calls: &[ToolCall],
        state: &mut RunLoopState,
    ) {
        let parsed = self.executor.parse_calls(tool_calls);
        let spans = self.emit_tool_starts(&parsed).await;
        let results = self.executor.execute_calls(&parsed, state.deadline()).await;
        self.apply_tool_results(results, &spans).await;

        // ── steering check ──
        if let Some(ref source) = self.steering_source {
            let iteration = self.iteration.load(Ordering::Relaxed);
            if let SteeringDecision::Redirect(msgs) = source.check_steering(iteration).await {
                for msg in msgs {
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

        let invoked: Vec<String> = parsed.iter().map(|p| p.call.name.clone()).collect();
        self.ctx.tool_view.note_invoked_batch(&invoked);
        self.ctx.tool_view.advance();
    }

    async fn emit_tool_starts(
        &mut self,
        parsed_calls: &[ParsedToolCall],
    ) -> HashMap<String, TraceSpan> {
        let mut spans = HashMap::new();
        for p in parsed_calls {
            let span = self.trace.start_span(
                p.kind.as_str(),
                &p.call.name,
                &self.loop_span_id,
                "",
                &SpanMeta::ToolStarted {
                    tool_call_id: p.call.id.clone(),
                    arguments: p.arguments.clone(),
                }
                .to_json(),
                "tool/skill execution started",
            );
            spans.insert(p.call.id.clone(), span);
            self.ctx.messages.push(Message::operation_event(
                p.kind.as_str(),
                &p.call.name,
                "started",
                serde_json::json!({"tool_call_id": p.call.id, "arguments": p.arguments}),
            ));
            self.emit(Event::ToolStart {
                tool_call_id: p.call.id.clone(),
                name: p.call.name.clone(),
                arguments: p.arguments.clone(),
            })
            .await;
            let tool_ctx = server_log::ServerCtx::new(
                &self.ctx.trace_id,
                &self.ctx.run_id,
                &self.ctx.session_id,
                &self.ctx.agent_id,
                self.iteration.load(std::sync::atomic::Ordering::Relaxed),
            );
            diagnostics::log_tool_started(tool_ctx, p);
            let turn = self.iteration.load(std::sync::atomic::Ordering::Relaxed);
            let payload = diagnostics::build_tool_started_payload(self.audit_payload(turn), p);
            self.emit_audit("tool.started", payload).await;
        }
        spans
    }

    async fn apply_tool_results(
        &mut self,
        results: Vec<DispatchOutcome>,
        spans: &HashMap<String, TraceSpan>,
    ) {
        for outcome in results {
            let p = &outcome.parsed;
            let meta = outcome.result.operation().clone();
            let (output, success, error_text) = match &outcome.result {
                ToolCallResult::Success(out, _) => (out.clone(), true, None),
                ToolCallResult::ToolError(msg, _) | ToolCallResult::InfraError(msg, _) => {
                    if matches!(&outcome.result, ToolCallResult::InfraError(..)) {
                        diagnostics::log_tool_infra_error(&p.call.name, msg);
                    }
                    (format!("Error: {msg}"), false, Some(msg.clone()))
                }
            };

            if let Some(span) = spans.get(&p.call.id) {
                self.record_tool_span(span, p, &meta, success, error_text.as_deref())
                    .await;
            }

            self.emit(Event::ToolEnd {
                tool_call_id: p.call.id.clone(),
                name: p.call.name.clone(),
                success,
                output: output.clone(),
                operation: meta.clone(),
            })
            .await;
            let result_ctx = server_log::ServerCtx::new(
                &self.ctx.trace_id,
                &self.ctx.run_id,
                &self.ctx.session_id,
                &self.ctx.agent_id,
                self.iteration.load(std::sync::atomic::Ordering::Relaxed),
            );
            diagnostics::log_tool_result(
                result_ctx,
                p,
                &meta,
                success,
                error_text.as_deref(),
                output.len(),
            );
            let turn = self.iteration.load(std::sync::atomic::Ordering::Relaxed);
            let payload = diagnostics::build_tool_result_payload(
                self.audit_payload(turn),
                p,
                success,
                &output,
                error_text.as_deref(),
                &meta,
            );
            self.emit_audit(
                if success {
                    "tool.completed"
                } else {
                    "tool.failed"
                },
                payload,
            )
            .await;
            self.ctx.messages.push(
                Message::tool_result_with_operation(
                    &p.call.id,
                    &p.call.name,
                    &output,
                    success,
                    meta,
                )
                .with_run_id(self.ctx.run_id.to_string()),
            );
        }
    }

    async fn record_tool_span(
        &mut self,
        span: &TraceSpan,
        p: &ParsedToolCall,
        meta: &OperationMeta,
        success: bool,
        error_text: Option<&str>,
    ) {
        if success {
            span.complete(
                meta.duration_ms,
                0,
                0,
                0,
                0,
                0.0,
                &SpanMeta::ToolCompleted {
                    tool_call_id: p.call.id.clone(),
                    duration_ms: meta.duration_ms,
                    impact: meta.impact.clone(),
                    summary: meta.summary.clone(),
                }
                .to_json(),
                "tool/skill execution completed",
            )
            .await;
            self.ctx.messages.push(Message::operation_event(
                p.kind.as_str(),
                &p.call.name,
                "completed",
                serde_json::json!({"tool_call_id": p.call.id, "duration_ms": meta.duration_ms}),
            ));
        } else {
            let err_full = error_text.unwrap_or_default();
            let err = crate::base::truncate_bytes_on_char_boundary(err_full, MAX_SPAN_ERROR);
            span.fail(
                meta.duration_ms,
                "tool_error",
                &err,
                &SpanMeta::ToolFailed {
                    tool_call_id: p.call.id.clone(),
                    duration_ms: meta.duration_ms,
                    error: err.clone(),
                    impact: meta.impact.clone(),
                    summary: meta.summary.clone(),
                }
                .to_json(),
                "tool/skill execution failed",
            )
            .await;
            self.ctx.messages.push(Message::operation_event(
                p.kind.as_str(),
                &p.call.name,
                "failed",
                serde_json::json!({"tool_call_id": p.call.id, "duration_ms": meta.duration_ms, "error": err}),
            ));
        }
    }

    pub(super) async fn try_compact(&mut self, state: &mut RunLoopState) {
        let total_tokens = prompt_projection::count_prompt_tokens(&self.ctx.messages);
        let max_tokens = state.max_context_tokens();
        let level = pressure::assess(total_tokens, max_tokens);

        // Elevated / High: extract memories before compaction
        if matches!(level, PressureLevel::Elevated | PressureLevel::High) {
            if let Some(ref mem) = self.memory {
                let transcript =
                    crate::kernel::run::compaction::build_transcript_from(&self.ctx.messages);
                let result = mem
                    .extract_and_save(
                        &transcript,
                        &self.ctx.user_id,
                        &self.ctx.agent_id,
                        self.cancel.clone(),
                    )
                    .await;
                if result.facts_written > 0 {
                    self.emit(Event::MemoryExtracted {
                        facts_written: result.facts_written,
                    })
                    .await;
                }
                state.add_token_usage(&result.token_usage);
            }
        }

        // High / Critical: run compaction
        if matches!(level, PressureLevel::High | PressureLevel::Critical) {
            if let Some(info) = self
                .compactor
                .compact(&mut self.ctx.messages, max_tokens, self.ctx.run_id.as_ref())
                .await
            {
                state.add_token_usage(&info.token_usage);
                self.emit(Event::CompactionDone {
                    messages_before: info.messages_before,
                    messages_after: info.messages_after,
                    summary_len: info.summary_len,
                })
                .await;
                if let Some(checkpoint) = info.checkpoint {
                    self.latest_checkpoint = Some(checkpoint);
                }
            }
        }
    }
}
