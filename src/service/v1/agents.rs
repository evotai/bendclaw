use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Serialize;

use super::common::ListQuery;
use super::common::Paginated;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;

#[derive(Serialize)]
pub struct AgentEntry {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct AgentDetail {
    pub agent_id: String,
    pub display_name: String,
    pub description: String,
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
    let agent_ids = state.runtime.databases().list_agent_ids().await?;
    let total = agent_ids.len() as u64;
    let offset = q.offset() as usize;
    let limit = q.limit() as usize;
    let page_ids: Vec<_> = agent_ids.into_iter().skip(offset).take(limit).collect();
    let entries = fetch_agent_entries(&state, &page_ids).await;
    Ok(Json(Paginated::new(entries, &q, total)))
}

/// GET /v1/agents/{agent_id}
pub async fn get_agent(
    State(state): State<AppState>,
    _ctx: RequestContext,
    Path(agent_id): Path<String>,
) -> Result<Json<AgentDetail>> {
    let db_name = state.runtime.agent_database_name(&agent_id);
    let exists = state.runtime.databases().database_exists(&db_name).await?;
    if !exists {
        return Err(ServiceError::AgentNotFound(format!(
            "agent '{agent_id}' not found"
        )));
    }

    let config_store = state.runtime.agent_config_store(&agent_id)?;
    let record = config_store
        .get(&agent_id)
        .await
        .map_err(ServiceError::from)?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("agent '{agent_id}' not found")))?;
    Ok(Json(AgentDetail {
        agent_id: record.agent_id,
        display_name: record.display_name,
        description: record.description,
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
    let db_name = state.runtime.agent_database_name(&agent_id);
    state
        .runtime
        .database()
        .exec(&format!("DROP DATABASE IF EXISTS `{db_name}`"))
        .await
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "deleted": agent_id })))
}

async fn fetch_agent_entries(state: &AppState, agent_ids: &[String]) -> Vec<AgentEntry> {
    use futures::stream::StreamExt;
    let tasks: Vec<_> = agent_ids
        .iter()
        .map(|id| {
            let runtime = state.runtime.clone();
            let id = id.clone();
            async move {
                let (real_id, display_name, description) = match runtime.agent_config_store(&id) {
                    Ok(store) => match store.get_any().await {
                        Ok(Some(r)) => (r.agent_id, r.display_name, r.description),
                        _ => (id.clone(), String::new(), String::new()),
                    },
                    Err(_) => (id.clone(), String::new(), String::new()),
                };
                AgentEntry {
                    agent_id: real_id,
                    display_name,
                    description,
                }
            }
        })
        .collect();
    futures::stream::iter(tasks)
        .buffer_unordered(10)
        .collect()
        .await
}
