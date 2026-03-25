use super::http::SessionResponse;
use super::http::SessionsQuery;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::Paginated;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionRepo;

pub(super) async fn list_sessions(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    q: SessionsQuery,
) -> Result<Paginated<SessionResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = SessionRepo::new(pool);
    let total = repo
        .count_by_user_search(user_id, q.search.as_deref())
        .await?;
    let order = format!("updated_at {}", q.list.order());
    let rows = repo
        .list_by_user_search(
            user_id,
            q.search.as_deref(),
            &order,
            q.list.limit() as u64,
            q.list.offset() as u64,
        )
        .await?;
    let data = rows.into_iter().map(to_response).collect();
    Ok(Paginated::new(data, &q.list, total))
}

pub(super) async fn get_session(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
) -> Result<SessionResponse> {
    let record = load_session_record(state, agent_id, session_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("session '{session_id}' not found")))?;
    Ok(to_response(record))
}

pub(super) async fn load_session_record(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
) -> Result<Option<SessionRecord>> {
    Ok(state
        .runtime
        .session_lifecycle()
        .load_session(agent_id, session_id)
        .await?)
}

pub(super) async fn create_session(
    state: &AppState,
    agent_id: &str,
    user_id: &str,
    title: Option<&str>,
    session_state: Option<&serde_json::Value>,
) -> Result<SessionResponse> {
    let record = state
        .runtime
        .session_lifecycle()
        .create_direct(agent_id, user_id, title, session_state, None)
        .await?;
    Ok(to_response(record))
}

pub(super) async fn update_session(
    state: &AppState,
    session_id: &str,
    existing: &SessionRecord,
    title: Option<String>,
    session_state: Option<serde_json::Value>,
) -> Result<SessionResponse> {
    let title = title.as_deref().unwrap_or(&existing.title);
    let session_state = session_state.as_ref().unwrap_or(&existing.session_state);
    let record = state
        .runtime
        .upsert_session(
            &existing.agent_id,
            session_id,
            &existing.user_id,
            Some(title),
            Some(session_state),
            Some(&existing.meta),
        )
        .await?;
    Ok(to_response(record))
}

pub(super) async fn delete_session(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
) -> Result<()> {
    state.runtime.delete_session(agent_id, session_id).await?;
    Ok(())
}

fn to_response(r: SessionRecord) -> SessionResponse {
    SessionResponse {
        id: r.id,
        agent_id: r.agent_id,
        user_id: r.user_id,
        title: r.title,
        base_key: r.base_key,
        replaced_by_session_id: r.replaced_by_session_id,
        reset_reason: r.reset_reason,
        session_state: r.session_state,
        meta: r.meta,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}
