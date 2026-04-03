//! Trace spans + audit + diagnostics for tool execution.
//!
//! Real-time sink — on_tool_start fires before execution, on_tool_end fires after.

use std::sync::Arc;

use tokio::sync::mpsc;

use super::diagnostics;
use super::parsed_tool_call::ParsedToolCall;
use super::turn_context::TurnContext;
use crate::execution::event::Event;
use crate::observability::audit;
use crate::observability::server_log;
use crate::tools::run_labels::RunLabels;
use crate::tools::OperationMeta;
use crate::traces::SpanMeta;
use crate::traces::Trace;
use crate::traces::TraceSpan;

/// Max bytes for span error messages stored in trace DB.
const MAX_SPAN_ERROR: usize = 2_000;

pub struct ExecutionRecorder {
    labels: Arc<RunLabels>,
    trace: Trace,
    tx: mpsc::Sender<Event>,
}

impl ExecutionRecorder {
    pub fn new(labels: Arc<RunLabels>, trace: Trace, tx: mpsc::Sender<Event>) -> Self {
        Self { labels, trace, tx }
    }

    pub fn labels(&self) -> Arc<RunLabels> {
        self.labels.clone()
    }

    // ── start ────────────────────────────────────────────────────────────

    pub fn start_tool_span(&self, parsed: &ParsedToolCall, tc: &TurnContext) -> TraceSpan {
        self.trace.start_span(
            parsed.kind_str(),
            &parsed.call.name,
            &tc.loop_span_id,
            "",
            &SpanMeta::ToolStarted {
                tool_call_id: parsed.call.id.clone(),
                arguments: parsed.arguments.clone(),
            }
            .to_json(),
            "tool/skill execution started",
        )
    }

    pub fn log_tool_started(&self, parsed: &ParsedToolCall, tc: &TurnContext) {
        diagnostics::log_tool_started(self.server_ctx(tc.turn), parsed);
    }

    pub async fn audit_tool_started(&self, parsed: &ParsedToolCall, tc: &TurnContext) {
        let payload = diagnostics::build_tool_started_payload(self.audit_payload(tc.turn), parsed);
        self.emit_audit("tool.started", payload).await;
    }

    // ── end ──────────────────────────────────────────────────────────────

    pub async fn complete_tool_span(
        &self,
        span: &TraceSpan,
        parsed: &ParsedToolCall,
        meta: &OperationMeta,
    ) {
        span.complete(
            meta.duration_ms,
            0,
            0,
            0,
            0,
            0.0,
            &SpanMeta::ToolCompleted {
                tool_call_id: parsed.call.id.clone(),
                duration_ms: meta.duration_ms,
                impact: meta.impact.clone(),
                summary: meta.summary.clone(),
            }
            .to_json(),
            "tool/skill execution completed",
        )
        .await;
    }

    pub async fn fail_tool_span(
        &self,
        span: &TraceSpan,
        parsed: &ParsedToolCall,
        meta: &OperationMeta,
        error: String,
    ) {
        let err = crate::types::truncate_bytes_on_char_boundary(&error, MAX_SPAN_ERROR);
        span.fail(
            meta.duration_ms,
            "tool_error",
            &err,
            &SpanMeta::ToolFailed {
                tool_call_id: parsed.call.id.clone(),
                duration_ms: meta.duration_ms,
                error: err.clone(),
                impact: meta.impact.clone(),
                summary: meta.summary.clone(),
            }
            .to_json(),
            "tool/skill execution failed",
        )
        .await;
    }

    pub fn log_tool_result(
        &self,
        parsed: &ParsedToolCall,
        meta: &OperationMeta,
        tc: &TurnContext,
        success: bool,
        error: Option<String>,
        output_len: usize,
    ) {
        diagnostics::log_tool_result(
            self.server_ctx(tc.turn),
            parsed,
            meta,
            success,
            error.as_deref(),
            output_len,
        );
    }

    pub fn log_tool_infra_error(&self, name: &str, error: &str) {
        diagnostics::log_tool_infra_error(name, error);
    }

    pub async fn audit_tool_ended(
        &self,
        parsed: &ParsedToolCall,
        tc: &TurnContext,
        success: bool,
        output: String,
        error: Option<String>,
        meta: &OperationMeta,
    ) {
        let payload = diagnostics::build_tool_result_payload(
            self.audit_payload(tc.turn),
            parsed,
            success,
            &output,
            error.as_deref(),
            meta,
        );
        let event_name = if success {
            "tool.completed"
        } else {
            "tool.failed"
        };
        self.emit_audit(event_name, payload).await;
    }

    // ── private helpers ──────────────────────────────────────────────────

    fn server_ctx(&self, turn: u32) -> server_log::ServerCtx<'_> {
        server_log::ServerCtx::new(
            &self.labels.trace_id,
            &self.labels.run_id,
            &self.labels.session_id,
            &self.labels.agent_id,
            turn,
        )
    }

    fn audit_payload(&self, turn: u32) -> serde_json::Map<String, serde_json::Value> {
        audit::base_payload(&self.server_ctx(turn))
    }

    async fn emit_audit(&self, name: &str, payload: serde_json::Map<String, serde_json::Value>) {
        let _ = self.tx.send(audit::event_from_map(name, payload)).await;
    }
}
