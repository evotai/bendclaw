use std::sync::Arc;

use super::contract::RuntimeInfra;
use crate::kernel::session::store::SessionStore;
use crate::kernel::trace::TraceWriter;
use crate::storage::Pool;

pub fn build_local_infra(
    store: Arc<dyn SessionStore>,
    tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    trace_writer: TraceWriter,
    persist_writer: crate::kernel::run::persist_op::PersistWriter,
) -> RuntimeInfra {
    RuntimeInfra {
        store,
        trace_factory: Arc::new(crate::kernel::trace::factory::NoopTraceFactory),
        tool_writer,
        trace_writer,
        persist_writer,
    }
}

pub fn build_cloud_infra(
    store: Arc<dyn SessionStore>,
    pool: Pool,
    tool_writer: crate::kernel::writer::tool_op::ToolWriter,
    trace_writer: TraceWriter,
    persist_writer: crate::kernel::run::persist_op::PersistWriter,
) -> RuntimeInfra {
    let trace_factory = Arc::new(crate::kernel::trace::factory::DbTraceFactory {
        trace_repo: Arc::new(crate::storage::dal::trace::repo::TraceRepo::new(
            pool.clone(),
        )),
        span_repo: Arc::new(crate::storage::dal::trace::repo::SpanRepo::new(pool)),
    });
    RuntimeInfra {
        store,
        trace_factory,
        tool_writer,
        trace_writer,
        persist_writer,
    }
}
