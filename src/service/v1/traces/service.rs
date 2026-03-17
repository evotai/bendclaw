use super::http::TraceDetailResponse;
use super::http::TraceResponse;
use super::http::TracesQuery;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::Paginated;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceListFilter;
use crate::storage::dal::trace::repo::TraceRepo;

pub async fn list_traces(
    state: &AppState,
    agent_id: &str,
    q: TracesQuery,
) -> Result<Paginated<TraceResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TraceRepo::new(pool);
    let filter = TraceListFilter {
        agent_id,
        session_id: q.session_id.as_deref(),
        run_id: q.run_id.as_deref(),
        user_id: q.user_id.as_deref(),
        status: q.status.as_deref(),
        start_time: q.start_time.as_deref(),
        end_time: q.end_time.as_deref(),
    };
    let total = repo.count_filtered(&filter).await?;
    let rows = repo
        .list_filtered(
            &filter,
            q.list.order(),
            q.list.limit() as u64,
            q.list.offset() as u64,
        )
        .await?;
    let data = rows.into_iter().map(to_response).collect();
    Ok(Paginated::new(data, &q.list, total))
}

pub async fn get_trace(
    state: &AppState,
    agent_id: &str,
    trace_id: &str,
) -> Result<TraceDetailResponse> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let trace_repo = TraceRepo::new(pool.clone());
    let span_repo = SpanRepo::new(pool);
    let trace = trace_repo.load(trace_id).await?.ok_or_else(|| {
        crate::service::error::ServiceError::AgentNotFound(format!("trace '{trace_id}' not found"))
    })?;
    let spans = span_repo.list_by_trace(trace_id).await?;
    Ok(TraceDetailResponse {
        trace: to_response(trace),
        spans,
    })
}

pub async fn traces_summary(
    state: &AppState,
    agent_id: &str,
) -> Result<crate::storage::AgentTraceSummary> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TraceRepo::new(pool);
    Ok(repo.summary_for_agent(agent_id).await?)
}

pub async fn list_spans(
    state: &AppState,
    agent_id: &str,
    trace_id: &str,
) -> Result<Vec<crate::storage::SpanRecord>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let span_repo = SpanRepo::new(pool);
    Ok(span_repo.list_by_trace(trace_id).await?)
}

pub async fn list_child_traces(
    state: &AppState,
    agent_id: &str,
    trace_id: &str,
    user_id: &str,
) -> Result<Vec<TraceResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TraceRepo::new(pool);
    let rows = repo.list_child_traces(trace_id, user_id).await?;
    Ok(rows.into_iter().map(to_response).collect())
}

fn to_response(r: TraceRecord) -> TraceResponse {
    TraceResponse {
        trace_id: r.trace_id,
        run_id: r.run_id,
        session_id: r.session_id,
        name: r.name,
        status: r.status,
        duration_ms: r.duration_ms,
        input_tokens: r.input_tokens,
        output_tokens: r.output_tokens,
        total_cost: r.total_cost,
        parent_trace_id: r.parent_trace_id,
        origin_node_id: r.origin_node_id,
        created_at: r.created_at,
    }
}
