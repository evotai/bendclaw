use super::record::TaskHistoryRecord;
use crate::storage::dal::logging::repo_error;
use crate::storage::dal::task::TaskDelivery;
use crate::storage::dal::task::TaskSchedule;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

const REPO: &str = "task_history";

#[derive(Clone)]
struct TaskHistoryMapper;

impl RowMapper for TaskHistoryMapper {
    type Entity = TaskHistoryRecord;

    fn columns(&self) -> &str {
        "id, task_id, run_id, task_name, schedule, prompt, status, output, error, duration_ms, delivery, delivery_status, delivery_error, user_id, executed_by_node_id, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<TaskHistoryRecord> {
        Ok(TaskHistoryRecord {
            id: sql::col(row, 0),
            task_id: sql::col(row, 1),
            run_id: sql::col_opt(row, 2),
            task_name: sql::col(row, 3),
            schedule: TaskSchedule::from_storage(&sql::col(row, 4), "task_history.schedule")?,
            prompt: sql::col(row, 5),
            status: sql::col(row, 6),
            output: sql::col_opt(row, 7),
            error: sql::col_opt(row, 8),
            duration_ms: sql::col_opt(row, 9).and_then(|s| s.parse().ok()),
            delivery: TaskDelivery::from_storage(&sql::col(row, 10), "task_history.delivery")?,
            delivery_status: sql::col_opt(row, 11),
            delivery_error: sql::col_opt(row, 12),
            user_id: sql::col(row, 13),
            executed_by_node_id: sql::col_opt(row, 14),
            created_at: sql::col(row, 15),
        })
    }
}

#[derive(Clone)]
pub struct TaskHistoryRepo {
    table: DatabendTable<TaskHistoryMapper>,
}

impl TaskHistoryRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "task_history", TaskHistoryMapper),
        }
    }

    pub async fn insert(&self, record: &TaskHistoryRecord) -> Result<()> {
        let schedule_expr = record.schedule.to_storage_expr()?;
        let delivery_expr = record.delivery.to_storage_expr()?;
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("task_id", SqlVal::Str(&record.task_id)),
                ("run_id", SqlVal::str_or_null(record.run_id.as_deref())),
                ("task_name", SqlVal::Str(&record.task_name)),
                ("schedule", SqlVal::Raw(&schedule_expr)),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("status", SqlVal::Str(&record.status)),
                ("output", SqlVal::str_or_null(record.output.as_deref())),
                ("error", SqlVal::str_or_null(record.error.as_deref())),
                ("duration_ms", match record.duration_ms {
                    Some(v) => SqlVal::Int(v as i64),
                    None => SqlVal::Null,
                }),
                ("delivery", SqlVal::Raw(&delivery_expr)),
                (
                    "delivery_status",
                    SqlVal::str_or_null(record.delivery_status.as_deref()),
                ),
                (
                    "delivery_error",
                    SqlVal::str_or_null(record.delivery_error.as_deref()),
                ),
                ("user_id", SqlVal::Str(&record.user_id)),
                (
                    "executed_by_node_id",
                    SqlVal::str_or_null(record.executed_by_node_id.as_deref()),
                ),
                ("created_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"history_id": record.id, "task_id": record.task_id}),
                error,
            );
        }
        result
    }

    pub async fn list_by_task(&self, task_id: &str, limit: u32) -> Result<Vec<TaskHistoryRecord>> {
        let result = self
            .table
            .list(
                &[Where("task_id", SqlVal::Str(task_id))],
                "created_at DESC",
                limit as u64,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_task",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }
}
