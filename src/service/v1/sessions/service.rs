use super::http::SessionResponse;
use super::http::SessionsQuery;
use crate::base::new_session_id;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::Paginated;
use crate::storage::dal::session::record::SessionRecord;
use crate::storage::dal::session::repo::SessionRepo;
use crate::storage::sql;

pub(super) async fn list_sessions(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    q: SessionsQuery,
) -> Result<Paginated<SessionResponse>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let uid = sql::escape(user_id);
    let mut cond = format!("user_id = '{uid}'");
    if let Some(ref s) = q.search {
        let escaped = sql::escape(s);
        cond.push_str(&format!(" AND title LIKE '%{escaped}%'"));
    }
    let total = count_u64(
        &pool,
        &format!("SELECT COUNT(*) FROM sessions WHERE {cond}"),
    )
    .await;
    let order = format!("updated_at {}", q.list.order());
    let data_sql = format!(
        "SELECT id, agent_id, user_id, title, PARSE_JSON(session_state), PARSE_JSON(meta), \
         TO_VARCHAR(created_at), TO_VARCHAR(updated_at) \
         FROM sessions WHERE {cond} ORDER BY {order} LIMIT {} OFFSET {}",
        q.list.limit(),
        q.list.offset()
    );
    let rows = pool.query_all(&data_sql).await?;
    let data = rows
        .iter()
        .map(|r| SessionResponse {
            id: sql::col(r, 0),
            agent_id: sql::col(r, 1),
            user_id: sql::col(r, 2),
            title: sql::col(r, 3),
            session_state: parse_variant(&sql::col(r, 4)),
            meta: parse_variant(&sql::col(r, 5)),
            created_at: sql::col(r, 6),
            updated_at: sql::col(r, 7),
        })
        .collect();
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
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = SessionRepo::new(pool);
    repo.load(session_id).await.map_err(ServiceError::from)
}

pub(super) async fn create_session(
    state: &AppState,
    agent_id: &str,
    user_id: &str,
    title: Option<&str>,
    session_state: Option<&serde_json::Value>,
) -> Result<String> {
    let session_id = new_session_id();
    state
        .runtime
        .upsert_session(agent_id, &session_id, user_id, title, session_state, None)
        .await?;
    Ok(session_id)
}

pub(super) async fn update_session(
    state: &AppState,
    session_id: &str,
    existing: &SessionRecord,
    title: Option<String>,
    session_state: Option<serde_json::Value>,
) -> Result<()> {
    let title = title.as_deref().unwrap_or(&existing.title);
    let session_state = session_state.as_ref().unwrap_or(&existing.session_state);
    state
        .runtime
        .upsert_session(
            &existing.agent_id,
            session_id,
            &existing.user_id,
            Some(title),
            Some(session_state),
            Some(&existing.meta),
        )
        .await
        .map_err(ServiceError::from)
}

pub(super) async fn delete_session(
    state: &AppState,
    agent_id: &str,
    session_id: &str,
) -> Result<()> {
    state
        .runtime
        .delete_session(agent_id, session_id)
        .await
        .map_err(ServiceError::from)
}

fn to_response(r: SessionRecord) -> SessionResponse {
    SessionResponse {
        id: r.id,
        agent_id: r.agent_id,
        user_id: r.user_id,
        title: r.title,
        session_state: r.session_state,
        meta: r.meta,
        created_at: r.created_at,
        updated_at: r.updated_at,
    }
}

fn parse_variant(raw: &str) -> serde_json::Value {
    if raw.trim().is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_str(raw).unwrap_or(serde_json::Value::Null)
}
