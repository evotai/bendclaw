//! TraceFactory — abstracts trace recorder creation for local/cloud.

use std::sync::Arc;

use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;
use crate::traces::recorder::TraceRecorder;
use crate::traces::TraceWriter;

pub trait TraceFactory: Send + Sync {
    fn create_recorder(
        &self,
        writer: &TraceWriter,
        trace_id: String,
        run_id: String,
        agent_id: String,
        session_id: String,
        user_id: String,
    ) -> TraceRecorder;
}

/// Cloud: creates real TraceRecorder backed by Databend repos.
pub struct DbTraceFactory {
    pub trace_repo: Arc<TraceRepo>,
    pub span_repo: Arc<SpanRepo>,
}

impl TraceFactory for DbTraceFactory {
    fn create_recorder(
        &self,
        writer: &TraceWriter,
        trace_id: String,
        run_id: String,
        agent_id: String,
        session_id: String,
        user_id: String,
    ) -> TraceRecorder {
        TraceRecorder::with_writer(
            writer.clone(),
            self.trace_repo.clone(),
            self.span_repo.clone(),
            trace_id,
            run_id,
            agent_id,
            session_id,
            user_id,
        )
    }
}

/// Local: creates a noop TraceRecorder that doesn't emit any trace ops.
pub struct NoopTraceFactory;

impl TraceFactory for NoopTraceFactory {
    fn create_recorder(
        &self,
        _writer: &TraceWriter,
        trace_id: String,
        run_id: String,
        agent_id: String,
        session_id: String,
        user_id: String,
    ) -> TraceRecorder {
        TraceRecorder::noop(trace_id, run_id, agent_id, session_id, user_id)
    }
}
