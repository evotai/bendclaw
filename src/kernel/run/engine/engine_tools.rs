use std::collections::HashMap;

use super::engine::Engine;
use crate::kernel::run::dispatcher::DispatchOutcome;
use crate::kernel::run::dispatcher::ParsedToolCall;
use crate::kernel::run::dispatcher::ToolCallResult;
use crate::kernel::run::event::Event;
use crate::kernel::run::run_loop::RunLoopState;
use crate::kernel::trace::SpanMeta;
use crate::kernel::trace::TraceSpan;
use crate::kernel::Message;
use crate::kernel::OperationMeta;
use crate::llm::message::ToolCall;
use crate::observability::log::run_log;
use crate::observability::log::slog;
use crate::observability::server_log;

/// Max bytes for span error messages stored in trace DB.
const MAX_SPAN_ERROR: usize = 2_000;

impl Engine {
    pub(super) async fn dispatch_tools(
        &mut self,
        tool_calls: &[ToolCall],
        state: &mut RunLoopState,
    ) {
        let parsed = self.dispatcher.parse_calls(tool_calls);
        let spans = self.emit_tool_starts(&parsed).await;
        let results = self
            .dispatcher
            .execute_calls(&parsed, state.deadline())
            .await;
        self.apply_tool_results(results, &spans).await;
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
            run_log!(info, tool_ctx, "tool", "started",
                msg = format!("    tool [{}] started", p.call.name),
                tool_name = %p.call.name,
                tool_kind = %p.kind.as_str(),
                bytes = p.arguments.to_string().len() as u64,
                tool_call_id = %p.call.id,
            );
            let turn = self.iteration.load(std::sync::atomic::Ordering::Relaxed);
            let mut payload = self.audit_payload(turn);
            payload.insert(
                "tool_call_id".to_string(),
                serde_json::json!(p.call.id.clone()),
            );
            payload.insert(
                "tool_name".to_string(),
                serde_json::json!(p.call.name.clone()),
            );
            payload.insert(
                "arguments".to_string(),
                serde_json::json!(p.arguments.clone()),
            );
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
                        slog!(error, "tool", "infra_error",
                            tool = %p.call.name,
                            error = %msg,
                        );
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
            if success {
                run_log!(info, result_ctx, "tool", "completed",
                    msg = format!("    tool [{}] completed {}ms", p.call.name, meta.duration_ms),
                    tool_name = %p.call.name,
                    tool_kind = %p.kind.as_str(),
                    summary = %meta.summary,
                    elapsed_ms = meta.duration_ms,
                    bytes = output.len() as u64,
                    tool_call_id = %p.call.id,
                );
            } else {
                run_log!(error, result_ctx, "tool", "failed",
                    msg = format!("    tool [{}] failed", p.call.name),
                    tool_name = %p.call.name,
                    tool_kind = %p.kind.as_str(),
                    error = %error_text.as_deref().unwrap_or(""),
                    summary = %meta.summary,
                    elapsed_ms = meta.duration_ms,
                    bytes = output.len() as u64,
                    tool_call_id = %p.call.id,
                );
            }
            let turn = self.iteration.load(std::sync::atomic::Ordering::Relaxed);
            let mut payload = self.audit_payload(turn);
            payload.insert(
                "tool_call_id".to_string(),
                serde_json::json!(p.call.id.clone()),
            );
            payload.insert(
                "tool_name".to_string(),
                serde_json::json!(p.call.name.clone()),
            );
            payload.insert("success".to_string(), serde_json::json!(success));
            payload.insert("output".to_string(), serde_json::json!(output.clone()));
            payload.insert("error".to_string(), serde_json::json!(error_text));
            payload.insert("operation".to_string(), serde_json::json!(meta.clone()));
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
        if let Some(info) = self
            .compactor
            .compact(
                &mut self.ctx.messages,
                state.max_context_tokens(),
                self.ctx.run_id.as_ref(),
            )
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
