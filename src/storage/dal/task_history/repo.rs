use super::record::TaskHistoryRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "task_history";

#[derive(Clone)]
struct TaskHistoryMapper;

impl RowMapper for TaskHistoryMapper {
    type Entity = TaskHistoryRecord;

    fn columns(&self) -> &str {
        "id, task_id, run_id, task_name, schedule_kind, cron_expr, prompt, status, output, error, duration_ms, webhook_url, webhook_status, webhook_error, executed_by_instance_id, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> TaskHistoryRecord {
        TaskHistoryRecord {
            id: sql::col(row, 0),
            task_id: sql::col(row, 1),
            run_id: sql::col_opt(row, 2),
            task_name: sql::col(row, 3),
            schedule_kind: sql::col(row, 4),
            cron_expr: sql::col_opt(row, 5),
            prompt: sql::col(row, 6),
            status: sql::col(row, 7),
            output: sql::col_opt(row, 8),
            error: sql::col_opt(row, 9),
            duration_ms: sql::col_opt(row, 10).and_then(|s| s.parse().ok()),
            webhook_url: sql::col_opt(row, 11),
            webhook_status: sql::col_opt(row, 12),
            webhook_error: sql::col_opt(row, 13),
            executed_by_instance_id: sql::col_opt(row, 14),
            created_at: sql::col(row, 15),
        }
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
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("task_id", SqlVal::Str(&record.task_id)),
                ("run_id", SqlVal::str_or_null(record.run_id.as_deref())),
                ("task_name", SqlVal::Str(&record.task_name)),
                ("schedule_kind", SqlVal::Str(&record.schedule_kind)),
                (
                    "cron_expr",
                    SqlVal::str_or_null(record.cron_expr.as_deref()),
                ),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("status", SqlVal::Str(&record.status)),
                ("output", SqlVal::str_or_null(record.output.as_deref())),
                ("error", SqlVal::str_or_null(record.error.as_deref())),
                ("duration_ms", match record.duration_ms {
                    Some(v) => SqlVal::Int(v as i64),
                    None => SqlVal::Null,
                }),
                (
                    "webhook_url",
                    SqlVal::str_or_null(record.webhook_url.as_deref()),
                ),
                (
                    "webhook_status",
                    SqlVal::str_or_null(record.webhook_status.as_deref()),
                ),
                (
                    "webhook_error",
                    SqlVal::str_or_null(record.webhook_error.as_deref()),
                ),
                (
                    "executed_by_instance_id",
                    SqlVal::str_or_null(record.executed_by_instance_id.as_deref()),
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
