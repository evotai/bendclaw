use super::http::CreateFeedbackRequest;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::feedback::FeedbackRecord;
use crate::storage::dal::feedback::FeedbackRepo;
use crate::types::new_id;

pub(super) async fn list_feedback(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<FeedbackRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = FeedbackRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(&pool, "SELECT COUNT(*) FROM feedback").await;
    Ok((records, total))
}

pub(super) async fn create_feedback(
    state: &AppState,
    agent_id: &str,
    req: CreateFeedbackRequest,
) -> Result<FeedbackRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = FeedbackRepo::new(pool);
    let record = FeedbackRecord {
        id: new_id(),
        agent_id: agent_id.to_string(),
        session_id: req.session_id.unwrap_or_default(),
        run_id: req.run_id.unwrap_or_default(),
        user_id: String::new(),
        scope: "shared".to_string(),
        created_by: String::new(),
        rating: req.rating,
        comment: req.comment.unwrap_or_default(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    Ok(record)
}

pub(super) async fn delete_feedback(
    state: &AppState,
    agent_id: &str,
    feedback_id: &str,
) -> Result<String> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = FeedbackRepo::new(pool);
    repo.delete(feedback_id).await?;
    Ok(feedback_id.to_string())
}
