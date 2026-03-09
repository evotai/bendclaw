use super::http::CreateLearningRequest;
use crate::base::new_id;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::learning::repo::LearningRepo;
use crate::storage::LearningRecord;

pub(super) async fn list_learnings(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<LearningRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list_by_agent(agent_id, limit).await?;
    let aid = crate::storage::sql::escape(agent_id);
    let total = count_u64(
        &pool,
        &format!("SELECT COUNT(*) FROM learnings WHERE agent_id = '{aid}'"),
    )
    .await;
    Ok((records, total))
}

pub(super) async fn search_learnings(
    state: &AppState,
    agent_id: &str,
    query: &str,
    limit: Option<u32>,
) -> Result<Vec<LearningRecord>> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = LearningRepo::new(pool);
    let limit = limit.unwrap_or(10).min(100);
    Ok(repo.search(agent_id, query, limit).await?)
}

pub(super) async fn create_learning(
    state: &AppState,
    user_id: &str,
    agent_id: &str,
    req: CreateLearningRequest,
) -> Result<LearningRecord> {
    let record = LearningRecord {
        id: new_id(),
        agent_id: agent_id.to_string(),
        user_id: user_id.to_string(),
        session_id: req.session_id,
        title: req.title,
        content: req.content,
        tags: req.tags.join(","),
        source: req.source,
        created_at: String::new(),
        updated_at: String::new(),
    };
    state
        .runtime
        .create_learning(agent_id, record.clone())
        .await?;
    Ok(record)
}

pub(super) async fn delete_learning(
    state: &AppState,
    agent_id: &str,
    learning_id: &str,
) -> Result<String> {
    state.runtime.delete_learning(agent_id, learning_id).await?;
    Ok(learning_id.to_string())
}
