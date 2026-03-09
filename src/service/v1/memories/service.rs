use super::http::CreateMemoryRequest;
use super::http::SearchMemoryRequest;
use crate::kernel::agent_store::memory_store::MemoryEntry;
use crate::kernel::agent_store::memory_store::MemoryResult;
use crate::kernel::agent_store::memory_store::SearchOpts;
use crate::kernel::agent_store::AgentStore;
use crate::service::error::Result;
use crate::service::error::ServiceError;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;

pub(super) async fn list_memories(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<MemoryEntry>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let storage = AgentStore::new(pool.clone(), state.runtime.llm());
    let entries = storage.memory_list(user_id, q.limit()).await?;
    let uid = crate::storage::sql::escape(user_id);
    let total = count_u64(
        &pool,
        &format!("SELECT COUNT(*) FROM memories WHERE user_id = '{uid}'"),
    )
    .await;
    Ok((entries, total))
}

pub(super) async fn get_memory(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    memory_id: &str,
) -> Result<MemoryEntry> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let storage = AgentStore::new(pool, state.runtime.llm());
    storage
        .memory_get_by_id(user_id, memory_id)
        .await?
        .ok_or_else(|| ServiceError::AgentNotFound(format!("memory '{memory_id}' not found")))
}

pub(super) async fn search_memories(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: SearchMemoryRequest,
) -> Result<Vec<MemoryResult>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let storage = AgentStore::new(pool, state.runtime.llm());
    let opts = SearchOpts {
        max_results: req.max_results.unwrap_or(10),
        include_shared: req.include_shared.unwrap_or(true),
        session_id: req.session_id,
        min_score: req.min_score.unwrap_or(0.0),
    };
    Ok(storage.memory_search(&req.query, user_id, opts).await?)
}

pub(super) async fn create_memory(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateMemoryRequest,
) -> Result<MemoryEntry> {
    let entry = MemoryEntry {
        id: crate::kernel::new_id(),
        user_id: user_id.to_string(),
        scope: crate::kernel::agent_store::memory_store::parse_scope(&req.scope),
        session_id: req.session_id,
        key: req.key,
        content: req.content,
        created_at: String::new(),
        updated_at: String::new(),
    };
    state
        .runtime
        .create_memory(agent_id, user_id, entry.clone())
        .await?;
    Ok(entry)
}

pub(super) async fn delete_memory(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    memory_id: &str,
) -> Result<String> {
    state
        .runtime
        .delete_memory(agent_id, user_id, memory_id)
        .await?;
    Ok(memory_id.to_string())
}
