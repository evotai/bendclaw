use serde::Serialize;

use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskRecord;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::dal::task_history::TaskHistoryRecord;

#[derive(Debug, Clone, Serialize)]
pub struct TaskView {
    pub id: String,
    pub node_id: String,
    pub name: String,
    pub prompt: String,
    pub enabled: bool,
    pub status: String,
    pub schedule: TaskSchedule,
    pub delivery: TaskDelivery,
    pub user_id: String,
    pub scope: String,
    pub created_by: String,
    pub last_error: Option<String>,
    pub delete_after_run: bool,
    pub run_count: i32,
    pub last_run_at: String,
    pub next_run_at: Option<String>,
    pub lease_token: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl From<TaskRecord> for TaskView {
    fn from(record: TaskRecord) -> Self {
        Self {
            id: record.id,
            node_id: record.node_id,
            name: record.name,
            prompt: record.prompt,
            enabled: record.enabled,
            status: record.status,
            schedule: record.schedule,
            delivery: record.delivery,
            user_id: record.user_id,
            scope: record.scope,
            created_by: record.created_by,
            last_error: record.last_error,
            delete_after_run: record.delete_after_run,
            run_count: record.run_count,
            last_run_at: record.last_run_at,
            next_run_at: record.next_run_at,
            lease_token: record.lease_token,
            created_at: record.created_at,
            updated_at: record.updated_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskSummaryView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub status: String,
    pub schedule: TaskSchedule,
    pub next_run_at: Option<String>,
}

impl From<TaskRecord> for TaskSummaryView {
    fn from(record: TaskRecord) -> Self {
        Self {
            id: record.id,
            name: record.name,
            enabled: record.enabled,
            status: record.status,
            schedule: record.schedule,
            next_run_at: record.next_run_at,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskHistoryView {
    pub id: String,
    pub task_id: String,
    pub run_id: Option<String>,
    pub task_name: String,
    pub schedule: TaskSchedule,
    pub prompt: String,
    pub status: String,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration_ms: Option<i32>,
    pub delivery: TaskDelivery,
    pub delivery_status: Option<String>,
    pub delivery_error: Option<String>,
    pub executed_by_node_id: Option<String>,
    pub created_at: String,
}

impl From<TaskHistoryRecord> for TaskHistoryView {
    fn from(record: TaskHistoryRecord) -> Self {
        Self {
            id: record.id,
            task_id: record.task_id,
            run_id: record.run_id,
            task_name: record.task_name,
            schedule: record.schedule,
            prompt: record.prompt,
            status: record.status,
            output: record.output,
            error: record.error,
            duration_ms: record.duration_ms,
            delivery: record.delivery,
            delivery_status: record.delivery_status,
            delivery_error: record.delivery_error,
            executed_by_node_id: record.executed_by_node_id,
            created_at: record.created_at,
        }
    }
}
