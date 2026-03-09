use super::http::CreateTaskRequest;
use super::http::UpdateTaskRequest;
use crate::base::new_id;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::task::repo::TaskRepo;
use crate::storage::dal::task::TaskRecord;

pub(super) async fn list_tasks(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<TaskRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(
        &pool,
        "SELECT COUNT(*) FROM tasks",
    )
    .await;
    Ok((records, total))
}

pub(super) async fn create_task(
    state: &AppState,
    agent_id: &str,
    req: CreateTaskRequest,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool);
    let record = TaskRecord {
        id: new_id(),
        agentos_id: req.agentos_id,
        name: req.name,
        cron_expr: req.cron_expr,
        prompt: req.prompt,
        enabled: true,
        status: "idle".to_string(),
        last_run_at: String::new(),
        next_run_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    };
    repo.insert(&record).await?;
    Ok(record)
}

pub(super) async fn update_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    req: UpdateTaskRequest,
) -> Result<()> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool);
    repo.update(
        task_id,
        req.name.as_deref(),
        req.cron_expr.as_deref(),
        req.prompt.as_deref(),
        req.enabled,
    )
    .await?;
    Ok(())
}

pub(super) async fn delete_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
) -> Result<String> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool);
    repo.delete(task_id).await?;
    Ok(task_id.to_string())
}

pub(super) async fn toggle_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool);
    repo.toggle(task_id).await?;
    let record = repo
        .get(task_id)
        .await?
        .ok_or_else(|| crate::base::ErrorCode::not_found(format!("task {task_id} not found")))?;
    Ok(record)
}
