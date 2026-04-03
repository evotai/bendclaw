use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::common::ListQuery;
use super::common::Paginated;
use crate::server::context::RequestContext;
use crate::server::error::Result;
use crate::server::error::ServiceError;
use crate::server::state::AppState;

#[derive(Serialize)]
pub struct AgentEntry {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
    pub model: String,
    pub visibility: String,
    pub status: String,
    pub user_id: String,
}

#[derive(Serialize)]
pub struct AgentDetail {
    pub agent_id: String,
    pub system_prompt: String,
    pub identity: String,
    pub soul: String,
    pub token_limit_total: Option<u64>,
    pub token_limit_daily: Option<u64>,
}

/// GET /v1/agents
pub async fn list_agents(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<AgentEntry>>> {
    // Query registry table in evotai_meta database
    let pool = state
        .runtime
        .databases()
        .root_pool()
        .with_database("evotai_meta")
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    let sql = "SELECT agent_id, display_name, description, model, visibility, status, user_id FROM evotai_agents WHERE status = 'active' ORDER BY updated_at DESC";
    let rows = pool
        .query_all(sql)
        .await
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    let total = rows.len() as u64;
    let offset = q.offset() as usize;
    let limit = q.limit() as usize;
    let entries: Vec<AgentEntry> = rows
        .iter()
        .skip(offset)
        .take(limit)
        .map(|row| AgentEntry {
            agent_id: crate::storage::sql::col(row, 0),
            display_name: crate::storage::sql::col(row, 1),
            description: crate::storage::sql::col(row, 2),
            model: crate::storage::sql::col(row, 3),
            visibility: crate::storage::sql::col(row, 4),
            status: crate::storage::sql::col(row, 5),
            user_id: crate::storage::sql::col(row, 6),
        })
        .collect();
    Ok(Json(Paginated::new(entries, &q, total)))
}

/// GET /v1/agents/{agent_id}
pub async fn get_agent(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentDetail>> {
    let db_name = state.runtime.agent_database_name(&agent_id)?;
    let exists = state.runtime.databases().database_exists(&db_name).await?;
    if !exists {
        return Err(ServiceError::AgentNotFound(format!(
            "agent '{agent_id}' not found"
        )));
    }

    let config_store = state.runtime.agent_config_store(&agent_id)?;
    let record = config_store
        .get(&agent_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("agent '{agent_id}' not found")))?;
    Ok(Json(AgentDetail {
        agent_id: record.agent_id,
        system_prompt: record.system_prompt,
        identity: record.identity,
        soul: record.soul,
        token_limit_total: record.token_limit_total,
        token_limit_daily: record.token_limit_daily,
    }))
}

/// DELETE /v1/agents/{agent_id}
pub async fn delete_agent(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<serde_json::Value>> {
    // Soft-delete in registry
    let meta_pool = state
        .runtime
        .databases()
        .root_pool()
        .with_database("evotai_meta")
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    let aid = crate::storage::sql::escape(&agent_id);
    let _ = meta_pool.exec(&format!("UPDATE evotai_agents SET status = 'deleted', updated_at = NOW() WHERE agent_id = '{aid}'")).await;

    // Drop agent database
    let db_name = state.runtime.agent_database_name(&agent_id)?;
    state
        .runtime
        .database()
        .exec(&format!("DROP DATABASE IF EXISTS `{db_name}`"))
        .await
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": agent_id })))
}
