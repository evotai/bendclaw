//! Execution tracing — records structured traces and spans to Databend.

use std::sync::Arc;
use std::time::Instant;

use crate::kernel::new_id;
use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;
use crate::traces::writer::TraceOp;
use crate::traces::writer::TraceWriter;

#[derive(Clone)]
pub struct TraceRecorder {
    writer: TraceWriter,
    trace_repo: Arc<TraceRepo>,
    span_repo: Arc<SpanRepo>,
    pub trace_id: String,
    pub run_id: String,
    agent_id: String,
    session_id: String,
    user_id: String,
    parent_trace_id: String,
    origin_node_id: String,
}

impl TraceRecorder {
    pub fn new(
        trace_repo: Arc<TraceRepo>,
        span_repo: Arc<SpanRepo>,
        trace_id: impl Into<String>,
        run_id: impl Into<String>,
        agent_id: impl Into<String>,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self::with_writer(
            TraceWriter::spawn(),
            trace_repo,
            span_repo,
            trace_id,
            run_id,
            agent_id,
            session_id,
            user_id,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_writer(
        writer: TraceWriter,
        trace_repo: Arc<TraceRepo>,
        span_repo: Arc<SpanRepo>,
        trace_id: impl Into<String>,
        run_id: impl Into<String>,
        agent_id: impl Into<String>,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        Self {
            writer,
            trace_repo,
            span_repo,
            trace_id: trace_id.into(),
            run_id: run_id.into(),
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            user_id: user_id.into(),
            parent_trace_id: String::new(),
            origin_node_id: String::new(),
        }
    }

    /// Noop recorder — doesn't emit any trace ops. For local/test use.
    pub fn noop(
        trace_id: impl Into<String>,
        run_id: impl Into<String>,
        agent_id: impl Into<String>,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Self {
        let noop_pool = crate::storage::Pool::noop();
        Self {
            writer: TraceWriter::noop(),
            trace_repo: Arc::new(TraceRepo::new(noop_pool.clone())),
            span_repo: Arc::new(SpanRepo::new(noop_pool)),
            trace_id: trace_id.into(),
            run_id: run_id.into(),
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            user_id: user_id.into(),
            parent_trace_id: String::new(),
            origin_node_id: String::new(),
        }
    }

    /// Set parent trace context for distributed trace linking.
    pub fn with_parent_trace(
        mut self,
        parent_trace_id: impl Into<String>,
        origin_node_id: impl Into<String>,
    ) -> Self {
        self.parent_trace_id = parent_trace_id.into();
        self.origin_node_id = origin_node_id.into();
        self
    }

    /// Insert the top-level trace record (status=running). Fire-and-forget.
    pub fn start_trace(&self, name: &str) {
        self.writer.send(TraceOp::InsertTrace {
            repo: self.trace_repo.clone(),
            record: TraceRecord {
                trace_id: self.trace_id.clone(),
                run_id: self.run_id.clone(),
                session_id: self.session_id.clone(),
                agent_id: self.agent_id.clone(),
                user_id: self.user_id.clone(),
                name: name.to_string(),
                status: "running".to_string(),
                duration_ms: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_cost: 0.0,
                parent_trace_id: self.parent_trace_id.clone(),
                origin_node_id: self.origin_node_id.clone(),
                created_at: String::new(),
                updated_at: String::new(),
            },
        });
    }

    /// Mark the trace as completed with aggregated metrics. Fire-and-forget.
    pub fn complete_trace(
        &self,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_cost: f64,
    ) {
        self.writer.send(TraceOp::UpdateTraceCompleted {
            repo: self.trace_repo.clone(),
            trace_id: self.trace_id.clone(),
            duration_ms,
            input_tokens,
            output_tokens,
            total_cost,
        });
    }

    /// Mark the trace as failed. Fire-and-forget.
    pub fn fail_trace(&self, duration_ms: u64) {
        self.writer.send(TraceOp::UpdateTraceFailed {
            repo: self.trace_repo.clone(),
            trace_id: self.trace_id.clone(),
            duration_ms,
        });
    }

    /// Append a span record. Fire-and-forget via writer queue.
    #[allow(dead_code)]
    pub fn append_span(&self, record: SpanRecord) {
        self.writer.send(TraceOp::AppendSpan {
            repo: self.span_repo.clone(),
            record,
        });
    }

    /// Create a started span and persist it. Fire-and-forget DB write.
    pub fn started_span(
        &self,
        kind: &str,
        name: &str,
        parent: &str,
        model_role: &str,
        meta: &str,
        summary: &str,
    ) -> String {
        let span_id = new_id();
        self.writer.send(TraceOp::AppendSpan {
            repo: self.span_repo.clone(),
            record: SpanRecord {
                span_id: span_id.clone(),
                trace_id: self.trace_id.clone(),
                parent_span_id: parent.to_string(),
                name: name.to_string(),
                kind: kind.to_string(),
                model_role: model_role.to_string(),
                status: "started".to_string(),
                duration_ms: 0,
                ttft_ms: 0,
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                cost: 0.0,
                error_code: String::new(),
                error_message: String::new(),
                summary: summary.to_string(),
                meta: meta.to_string(),
                created_at: String::new(),
            },
        });
        span_id
    }

    #[allow(clippy::too_many_arguments)]
    pub fn completed_span(
        &self,
        span_id: &str,
        parent: &str,
        kind: &str,
        name: &str,
        model_role: &str,
        duration_ms: u64,
        ttft_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        cost: f64,
        meta: &str,
        summary: &str,
    ) {
        self.writer.send(TraceOp::AppendSpan {
            repo: self.span_repo.clone(),
            record: SpanRecord {
                span_id: span_id.to_string(),
                trace_id: self.trace_id.clone(),
                parent_span_id: parent.to_string(),
                name: name.to_string(),
                kind: kind.to_string(),
                model_role: model_role.to_string(),
                status: "completed".to_string(),
                duration_ms,
                ttft_ms,
                input_tokens,
                output_tokens,
                reasoning_tokens,
                cost,
                error_code: String::new(),
                error_message: String::new(),
                summary: summary.to_string(),
                meta: meta.to_string(),
                created_at: String::new(),
            },
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn failed_span(
        &self,
        span_id: &str,
        parent: &str,
        kind: &str,
        name: &str,
        model_role: &str,
        duration_ms: u64,
        error_code: &str,
        error_message: &str,
        meta: &str,
        summary: &str,
    ) {
        self.writer.send(TraceOp::AppendSpan {
            repo: self.span_repo.clone(),
            record: SpanRecord {
                span_id: span_id.to_string(),
                trace_id: self.trace_id.clone(),
                parent_span_id: parent.to_string(),
                name: name.to_string(),
                kind: kind.to_string(),
                model_role: model_role.to_string(),
                status: "failed".to_string(),
                duration_ms,
                ttft_ms: 0,
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                cost: 0.0,
                error_code: error_code.to_string(),
                error_message: error_message.to_string(),
                summary: summary.to_string(),
                meta: meta.to_string(),
                created_at: String::new(),
            },
        });
    }

    #[allow(dead_code)]
    pub fn cancelled_span(
        &self,
        span_id: &str,
        parent: &str,
        kind: &str,
        name: &str,
        duration_ms: u64,
        summary: &str,
    ) {
        self.writer.send(TraceOp::AppendSpan {
            repo: self.span_repo.clone(),
            record: SpanRecord {
                span_id: span_id.to_string(),
                trace_id: self.trace_id.clone(),
                parent_span_id: parent.to_string(),
                name: name.to_string(),
                kind: kind.to_string(),
                model_role: String::new(),
                status: "cancelled".to_string(),
                duration_ms,
                ttft_ms: 0,
                input_tokens: 0,
                output_tokens: 0,
                reasoning_tokens: 0,
                cost: 0.0,
                error_code: "cancelled".to_string(),
                error_message: "operation cancelled".to_string(),
                summary: summary.to_string(),
                meta: "{}".to_string(),
                created_at: String::new(),
            },
        });
    }
}

// ── TraceSpan ─────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct TraceSpan {
    recorder: TraceRecorder,
    pub span_id: String,
    parent_id: String,
    kind: String,
    name: String,
    model_role: String,
    started_at: Instant,
}

impl TraceSpan {
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn complete(
        &self,
        duration_ms: u64,
        ttft_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        cost: f64,
        meta: &str,
        summary: &str,
    ) {
        self.recorder.completed_span(
            &self.span_id,
            &self.parent_id,
            &self.kind,
            &self.name,
            &self.model_role,
            duration_ms,
            ttft_ms,
            input_tokens,
            output_tokens,
            reasoning_tokens,
            cost,
            meta,
            summary,
        );
    }

    pub async fn fail(
        &self,
        duration_ms: u64,
        error_code: &str,
        error_message: &str,
        meta: &str,
        summary: &str,
    ) {
        self.recorder.failed_span(
            &self.span_id,
            &self.parent_id,
            &self.kind,
            &self.name,
            &self.model_role,
            duration_ms,
            error_code,
            error_message,
            meta,
            summary,
        );
    }
}

// ── Trace (engine-facing facade) ──────────────────────────────────────────────

#[derive(Clone)]
pub struct Trace {
    recorder: TraceRecorder,
}

impl Trace {
    pub fn new(recorder: TraceRecorder) -> Self {
        Self { recorder }
    }

    pub fn start_span(
        &self,
        kind: &str,
        name: &str,
        parent: &str,
        model_role: &str,
        meta: &str,
        summary: &str,
    ) -> TraceSpan {
        let span_id = self
            .recorder
            .started_span(kind, name, parent, model_role, meta, summary);
        TraceSpan {
            recorder: self.recorder.clone(),
            span_id,
            parent_id: parent.to_string(),
            kind: kind.to_string(),
            name: name.to_string(),
            model_role: model_role.to_string(),
            started_at: Instant::now(),
        }
    }
}
