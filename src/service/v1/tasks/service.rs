use super::http::CreateTaskRequest;
use super::http::UpdateTaskRequest;
use crate::base::new_id;
use crate::kernel::scheduler::executor::compute_next_run;
use crate::service::error::Result;
use crate::service::state::AppState;
use crate::service::v1::common::count_u64;
use crate::service::v1::common::ListQuery;
use crate::storage::dal::task::repo::TaskRepo;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task_history::repo::TaskHistoryRepo;
use crate::storage::dal::task_history::TaskHistoryRecord;

pub(super) async fn list_tasks(
    state: &AppState,
    agent_id: &str,
    q: &ListQuery,
) -> Result<(Vec<TaskRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list(limit).await?;
    let total = count_u64(&pool, "SELECT COUNT(*) FROM tasks").await;
    Ok((records, total))
}

pub(super) async fn create_task(
    state: &AppState,
    agent_id: &str,
    req: CreateTaskRequest,
) -> Result<TaskRecord> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskRepo::new(pool);
    let mut next_run_at = compute_next_run(&req.schedule_kind, &req.cron_expr, req.every_seconds);
    // For one-shot "at" tasks, use the specified at_time
    if req.schedule_kind == "at" {
        next_run_at = req.at_time.clone();
    }
    let record = TaskRecord {
        id: new_id(),
        agentos_id: req.agentos_id,
        name: req.name,
        cron_expr: req.cron_expr,
        prompt: req.prompt,
        enabled: true,
        status: "idle".to_string(),
        schedule_kind: req.schedule_kind,
        every_seconds: req.every_seconds,
        at_time: req.at_time.clone(),
        tz: req.tz,
        webhook_url: req.webhook_url,
        last_error: None,
        delete_after_run: req.delete_after_run,
        run_count: 0,
        last_run_at: String::new(),
        next_run_at,
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
    let repo = TaskRepo::new(pool.clone());

    // Recompute next_run_at if any schedule param changed
    let schedule_changed = req.schedule_kind.is_some()
        || req.cron_expr.is_some()
        || req.every_seconds.is_some()
        || req.at_time.is_some();

    let next_run_at = if schedule_changed {
        // Need current task to fill in unchanged fields
        let current = repo.get(task_id).await?.ok_or_else(|| {
            crate::base::ErrorCode::not_found(format!("task {task_id} not found"))
        })?;
        let kind = req
            .schedule_kind
            .as_deref()
            .unwrap_or(&current.schedule_kind);
        let cron = req.cron_expr.as_deref().unwrap_or(&current.cron_expr);
        let every = match req.every_seconds {
            Some(v) => v,
            None => current.every_seconds,
        };
        let mut next = compute_next_run(kind, cron, every);
        if kind == "at" {
            next = match &req.at_time {
                Some(v) => v.clone(),
                None => current.at_time.clone(),
            };
        }
        Some(next)
    } else {
        None
    };

    repo.update(
        task_id,
        req.name.as_deref(),
        req.cron_expr.as_deref(),
        req.prompt.as_deref(),
        req.enabled,
        req.schedule_kind.as_deref(),
        req.every_seconds,
        req.at_time.as_ref().map(|v| v.as_deref()),
        req.tz.as_ref().map(|v| v.as_deref()),
        req.webhook_url.as_ref().map(|v| v.as_deref()),
        req.delete_after_run,
        next_run_at.as_ref().map(|v| v.as_deref()),
    )
    .await?;
    Ok(())
}

pub(super) async fn delete_task(state: &AppState, agent_id: &str, task_id: &str) -> Result<String> {
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

pub(super) async fn list_task_history(
    state: &AppState,
    agent_id: &str,
    task_id: &str,
    q: &ListQuery,
) -> Result<(Vec<TaskHistoryRecord>, u64)> {
    let pool = state.runtime.databases().agent_pool(agent_id)?;
    let repo = TaskHistoryRepo::new(pool.clone());
    let limit = q.limit();
    let records = repo.list_by_task(task_id, limit).await?;
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
