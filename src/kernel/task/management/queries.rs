use crate::base::Result;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskRepo;
use crate::storage::dal::task_history::TaskHistoryRecord;
use crate::storage::dal::task_history::TaskHistoryRepo;
use crate::storage::pool::Pool;

pub async fn list_tasks(pool: &Pool, limit: u32) -> Result<Vec<TaskRecord>> {
    let repo = TaskRepo::new(pool.clone());
    repo.list(limit).await
}

pub async fn get_task(pool: &Pool, task_id: &str) -> Result<Option<TaskRecord>> {
    let repo = TaskRepo::new(pool.clone());
    repo.get(task_id).await
}

pub async fn list_task_history(
    pool: &Pool,
    task_id: &str,
    limit: u32,
) -> Result<Vec<TaskHistoryRecord>> {
    let repo = TaskHistoryRepo::new(pool.clone());
    repo.list_by_task(task_id, limit).await
}
