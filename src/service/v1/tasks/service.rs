use super::http::CreateTaskRequest;
use super::http::UpdateTaskRequest;
use crate::kernel::task::admin;
use crate::kernel::task::admin::CreateTaskParams;
use crate::kernel::task::admin::UpdateTaskParams;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task_history::TaskHistoryRecord;

pub(super) async fn list_tasks(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<TaskRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let limit = q.limit();
    let records = admin::list_tasks(&pool, limit).await?;
    let total = count_u64(&pool, "SELECT COUNT(*) FROM tasks").await;
    Ok((records, total))
}

pub(super) async fn create_task(
    state: &AppState,
    agent_id: &str,
    req: CreateTaskRequest,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let executor_instance_id = req
        .executor_instance_id
        .unwrap_or_else(|| state.runtime.config().instance_id.clone());
    let params = CreateTaskParams {
        executor_instance_id,
        name: req.name,
        prompt: req.prompt,
        schedule: req.schedule.into_task_schedule(),
        webhook_url: req.webhook_url,
        delete_after_run: req.delete_after_run,
    };
    let record = admin::create_task(&pool, params).await?;
    Ok(record)
}

pub(super) async fn update_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    req: UpdateTaskRequest,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let params = UpdateTaskParams {
        name: req.name,
        prompt: req.prompt,
        schedule: req.schedule.map(|s| s.into_task_schedule()),
        enabled: req.enabled,
        webhook_url: req.webhook_url,
        delete_after_run: req.delete_after_run,
    };
    let record = admin::update_task(&pool, task_id, params).await?;
    Ok(record)
}

pub(super) async fn delete_task(state: &AppState, agent_id: &str, task_id: &str) -> Result<String> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    admin::delete_task(&pool, task_id).await?;
    Ok(task_id.to_string())
}

pub(super) async fn toggle_task(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    Ok(admin::toggle_task(&pool, task_id).await?)
}

pub(super) async fn list_task_history(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    q: &ListQuery,
) -> Result<(Vec<TaskHistoryRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let limit = q.limit();
    let records = admin::list_task_history(&pool, task_id, limit).await?;
    let total = count_u64(
        &pool,
        &format!(
            "SELECT COUNT(*) FROM task_history WHERE task_id = '{}'",
            crate::storage::sql::escape(task_id)
        ),
    )
    .await;
    Ok((records, total))
}
