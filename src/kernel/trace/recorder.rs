//! Execution tracing — records structured traces and spans to Databend.

use std::sync::Arc;
use std::time::Instant;

use crate::base::Result;
use crate::kernel::new_id;
use crate::storage::dal::trace::record::SpanRecord;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;

#[derive(Clone)]
pub struct TraceRecorder {
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
        Self {
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

    /// Insert the top-level trace record (status=running).
    pub async fn start_trace(&self, name: &str) -> Result<()> {
        self.trace_repo
            .insert(&TraceRecord {
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
            })
            .await
    }

    /// Mark the trace as completed with aggregated metrics.
    pub async fn complete_trace(
        &self,
        duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        total_cost: f64,
    ) -> Result<()> {
        self.trace_repo
            .update_completed(
                &self.trace_id,
                duration_ms,
                input_tokens,
                output_tokens,
                total_cost,
            )
            .await
    }

    /// Mark the trace as failed.
    pub async fn fail_trace(&self, duration_ms: u64) -> Result<()> {
        self.trace_repo
            .update_failed(&self.trace_id, duration_ms)
            .await
    }

    /// Append a span record.
    #[allow(dead_code)]
    pub async fn append_span(&self, record: &SpanRecord) -> Result<()> {
        self.span_repo.append(record).await
    }

    /// Create a started span and persist it.
    pub async fn started_span(
        &self,
        kind: &str,
        name: &str,
        parent: &str,
        model_role: &str,
        meta: &str,
        summary: &str,
    ) -> Result<String> {
        let span_id = new_id();
        self.span_repo
            .append(&SpanRecord {
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
            })
            .await?;
        Ok(span_id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn completed_span(
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
    ) -> Result<()> {
        self.span_repo
            .append(&SpanRecord {
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
            })
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn failed_span(
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
    ) -> Result<()> {
        self.span_repo
            .append(&SpanRecord {
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
            })
            .await
    }

    #[allow(dead_code)]
    pub async fn cancelled_span(
        &self,
        span_id: &str,
        parent: &str,
        kind: &str,
        name: &str,
        duration_ms: u64,
        summary: &str,
    ) -> Result<()> {
        self.span_repo
            .append(&SpanRecord {
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
            })
            .await
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
        let _ = self
            .recorder
            .completed_span(
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
            )
            .await;
    }

    pub async fn fail(
        &self,
        duration_ms: u64,
        error_code: &str,
        error_message: &str,
        meta: &str,
        summary: &str,
    ) {
        let _ = self
            .recorder
            .failed_span(
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
            )
            .await;
    }
}

// ── Trace (engine-facing facade) ──────────────────────────────────────────────

#[derive(Clone)]
pub struct Trace {
    recorder: TraceRecorder,
}

impl Trace {
    pub(crate) fn new(recorder: TraceRecorder) -> Self {
        Self { recorder }
    }

    pub async fn start_span(
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
            .started_span(kind, name, parent, model_role, meta, summary)
            .await
            .unwrap_or_else(|_| new_id());
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::Trace;
    use super::TraceRecorder;
    use crate::storage::pool::DatabendClient;
    use crate::storage::pool::QueryResponse;
    use crate::storage::Pool;
    use crate::storage::SpanRepo;
    use crate::storage::TraceRepo;

    #[derive(Clone, Default)]
    struct RecordingClient {
        sqls: Arc<Mutex<Vec<String>>>,
    }

    impl RecordingClient {
        fn sqls(&self) -> Vec<String> {
            self.sqls.lock().expect("trace sqls lock").clone()
        }
    }

    #[async_trait]
    impl DatabendClient for RecordingClient {
        async fn query(
            &self,
            sql: &str,
            _database: Option<&str>,
        ) -> crate::base::Result<QueryResponse> {
            self.sqls
                .lock()
                .expect("trace sqls lock")
                .push(sql.to_string());
            Ok(QueryResponse {
                id: String::new(),
                state: "Succeeded".to_string(),
                error: None,
                data: Vec::new(),
                next_uri: None,
                final_uri: None,
                schema: Vec::new(),
            })
        }

        async fn page(&self, _uri: &str) -> crate::base::Result<QueryResponse> {
            unreachable!("trace recorder should not request pages")
        }

        async fn finalize(&self, _uri: &str) -> crate::base::Result<()> {
            Ok(())
        }
    }

    fn fake_pool(client: &RecordingClient) -> Pool {
        Pool::from_client("http://fake.local/v1", "default", Arc::new(client.clone()))
    }

    #[tokio::test]
    async fn trace_recorder_persists_trace_and_completed_span() {
        let client = RecordingClient::default();
        let pool = fake_pool(&client);
        let recorder = TraceRecorder::new(
            Arc::new(TraceRepo::new(pool.clone())),
            Arc::new(SpanRepo::new(pool)),
            "trace-1",
            "run-1",
            "agent-1",
            "session-1",
            "user-1",
        );

        recorder
            .start_trace("agent.run")
            .await
            .expect("start trace");
        let trace = Trace::new(recorder.clone());
        let span = trace
            .start_span("tool", "shell", "", "assistant", "{}", "echo hi")
            .await;
        span.complete(12, 3, 4, 5, 0, 0.25, "{}", "done").await;
        recorder
            .complete_trace(42, 10, 20, 0.5)
            .await
            .expect("complete trace");

        let sqls = client.sqls();
        assert!(sqls.iter().any(|sql| sql.contains("INSERT INTO traces")));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'started'")));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'completed'")));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("UPDATE traces SET status = 'completed'")));
    }

    #[tokio::test]
    async fn trace_recorder_persists_failed_and_cancelled_spans() {
        let client = RecordingClient::default();
        let pool = fake_pool(&client);
        let recorder = TraceRecorder::new(
            Arc::new(TraceRepo::new(pool.clone())),
            Arc::new(SpanRepo::new(pool)),
            "trace-2",
            "run-2",
            "agent-2",
            "session-2",
            "user-2",
        );

        let trace = Trace::new(recorder.clone());
        let span = trace
            .start_span("skill", "remote-tool", "", "assistant", "{}", "run skill")
            .await;
        span.fail(9, "oops", "failed to run", "{}", "broken").await;
        recorder
            .cancelled_span("span-cancelled", "", "tool", "shell", 7, "cancelled")
            .await
            .expect("cancelled span");
        recorder.fail_trace(99).await.expect("fail trace");

        let sqls = client.sqls();
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("INSERT INTO spans") && sql.contains("'failed'")));
        assert!(sqls.iter().any(|sql| {
            sql.contains("INSERT INTO spans")
                && sql.contains("'cancelled'")
                && sql.contains("operation cancelled")
        }));
        assert!(sqls
            .iter()
            .any(|sql| sql.contains("UPDATE traces SET status = 'failed'")));
    }
}
