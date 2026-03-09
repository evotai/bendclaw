use axum::extract::Path;
use axum::extract::Query;
use axum::extract::State;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use super::service;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::memory_store::MemoryScope;
use crate::service::context::RequestContext;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::ListQuery;
use crate::service::v1::common::Paginated;

#[derive(Serialize)]
pub struct MemoryResponse {
    pub id: String,
    pub scope: MemoryScope,
    pub session_id: Option<String>,
    pub key: String,
    pub content: String,
    pub created_at: String,
    pub updated_at: String,
}

fn to_response(e: MemoryEntry) -> MemoryResponse {
    MemoryResponse {
        id: e.id,
        scope: e.scope,
        session_id: e.session_id,
        key: e.key,
        content: e.content,
        created_at: e.created_at,
        updated_at: e.updated_at,
    }
}

#[derive(Deserialize)]
pub struct CreateMemoryRequest {
    pub key: String,
    pub content: String,
    #[serde(default = "default_scope")]
    pub scope: String,
    pub session_id: Option<String>,
}

fn default_scope() -> String {
    "user".to_string()
}

#[derive(Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub max_results: Option<u32>,
    pub include_shared: Option<bool>,
    pub session_id: Option<String>,
    pub min_score: Option<f32>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub id: String,
    pub key: String,
    pub content: String,
    pub scope: MemoryScope,
    pub score: f32,
    pub updated_at: String,
}

pub async fn create_memory(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<CreateMemoryRequest>,
) -> Result<Json<MemoryResponse>> {
    let entry = service::create_memory(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(to_response(entry)))
}

pub async fn list_memories(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> Result<Json<Paginated<MemoryResponse>>> {
    let (entries, total) = service::list_memories(&state, &ctx.user_id, &agent_id, &q).await?;
    Ok(Json(Paginated::new(
        entries.into_iter().map(to_response).collect(),
        &q,
        total,
    )))
}

pub async fn get_memory(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, memory_id)): Path<(String, String)>,
) -> Result<Json<MemoryResponse>> {
    let entry = service::get_memory(&state, &ctx.user_id, &agent_id, &memory_id).await?;
    Ok(Json(to_response(entry)))
}

pub async fn delete_memory(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path((agent_id, memory_id)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>> {
    let deleted = service::delete_memory(&state, &ctx.user_id, &agent_id, &memory_id).await?;
    Ok(Json(serde_json::json!({ "deleted": deleted })))
}

pub async fn search_memories(
    State(state): State<AppState>,
    ctx: RequestContext,
    Path(agent_id): Path<String>,
    Json(req): Json<SearchMemoryRequest>,
) -> Result<Json<Vec<SearchResult>>> {
    let results = service::search_memories(&state, &ctx.user_id, &agent_id, req).await?;
    Ok(Json(
        results
            .into_iter()
            .map(|r| SearchResult {
                id: r.id,
                key: r.key,
                content: r.content,
                scope: r.scope,
                score: r.score,
                updated_at: r.updated_at,
            })
            .collect(),
    ))
}
