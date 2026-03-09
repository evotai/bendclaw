use super::http::TraceDetailResponse;
use super::http::TraceResponse;
use super::http::TracesQuery;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::Paginated;
use crate::storage::dal::trace::record::TraceRecord;
use crate::storage::dal::trace::repo::SpanRepo;
use crate::storage::dal::trace::repo::TraceRepo;
use crate::storage::sql;

pub async fn list_traces(
    state: &AppState,
    agent_id: &str,
    q: TracesQuery,
) -> Result<Paginated<TraceResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let mut wheres = vec![format!("agent_id = '{}'", sql::escape(agent_id))];
    if let Some(ref sid) = q.session_id {
        wheres.push(format!("session_id = '{}'", sql::escape(sid)));
    }
    if let Some(ref rid) = q.run_id {
        wheres.push(format!("run_id = '{}'", sql::escape(rid)));
    }
    if let Some(ref uid) = q.user_id {
        wheres.push(format!("user_id = '{}'", sql::escape(uid)));
    }
    if let Some(ref st) = q.status {
        wheres.push(format!("status = '{}'", sql::escape(st)));
    }
    if let Some(ref t) = q.start_time {
        wheres.push(format!("created_at >= '{}'", sql::escape(t)));
    }
    if let Some(ref t) = q.end_time {
        wheres.push(format!("created_at <= '{}'", sql::escape(t)));
    }
    let cond = wheres.join(" AND ");
    let total = crate::service::v1::common::count_u64(
        &pool,
        &format!("SELECT COUNT(*) FROM traces WHERE {cond}"),
    )
    .await;
    let order = format!("created_at {}", q.list.order());
    let data_sql = format!(
        "SELECT trace_id, run_id, session_id, agent_id, user_id, name, status, duration_ms, \
         input_tokens, output_tokens, total_cost, TO_VARCHAR(created_at) \
         FROM traces WHERE {cond} ORDER BY {order} LIMIT {} OFFSET {}",
        q.list.limit(),
        q.list.offset()
    );
    let rows = pool.query_all(&data_sql).await?;
    let data = rows
        .iter()
        .map(|r| TraceResponse {
            trace_id: sql::col(r, 0),
            run_id: sql::col(r, 1),
            session_id: sql::col(r, 2),
            name: sql::col(r, 5),
            status: sql::col(r, 6),
            duration_ms: sql::col(r, 7).parse().unwrap_or(0),
            input_tokens: sql::col(r, 8).parse().unwrap_or(0),
            output_tokens: sql::col(r, 9).parse().unwrap_or(0),
            total_cost: sql::col(r, 10).parse().unwrap_or(0.0),
            created_at: sql::col(r, 11),
        })
        .collect();
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
    repo.summary_for_agent(agent_id).await.map_err(Into::into)
}

pub async fn list_spans(
    state: &AppState,
    agent_id: &str,
    trace_id: &str,
) -> Result<Vec<crate::storage::SpanRecord>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let span_repo = SpanRepo::new(pool);
    span_repo.list_by_trace(trace_id).await.map_err(Into::into)
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
        created_at: r.created_at,
    }
}
