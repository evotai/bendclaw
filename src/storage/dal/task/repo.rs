use super::record::TaskRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "task";

#[derive(Clone)]
struct TaskMapper;

impl RowMapper for TaskMapper {
    type Entity = TaskRecord;

    fn columns(&self) -> &str {
        "id, agentos_id, name, cron_expr, prompt, enabled, status, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> TaskRecord {
        let enabled_str = sql::col(row, 5);
        let enabled = enabled_str == "1" || enabled_str.eq_ignore_ascii_case("true");
        TaskRecord {
            id: sql::col(row, 0),
            agentos_id: sql::col(row, 1),
            name: sql::col(row, 2),
            cron_expr: sql::col(row, 3),
            prompt: sql::col(row, 4),
            enabled,
            status: sql::col(row, 6),
            last_run_at: sql::col(row, 7),
            next_run_at: sql::col(row, 8),
            created_at: sql::col(row, 9),
            updated_at: sql::col(row, 10),
        }
    }
}

#[derive(Clone)]
pub struct TaskRepo {
    table: DatabendTable<TaskMapper>,
}

impl TaskRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "tasks", TaskMapper),
        }
    }

    pub async fn insert(&self, record: &TaskRecord) -> Result<()> {
        let enabled_raw = if record.enabled { "true" } else { "false" };
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("agentos_id", SqlVal::Str(&record.agentos_id)),
                ("name", SqlVal::Str(&record.name)),
                ("cron_expr", SqlVal::Str(&record.cron_expr)),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("enabled", SqlVal::Raw(enabled_raw)),
                ("status", SqlVal::Str(&record.status)),
                ("last_run_at", SqlVal::Raw("NULL")),
                ("next_run_at", SqlVal::Raw("NULL")),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"task_id": record.id}),
                error,
            );
        }
        result
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<TaskRecord>> {
        let result = self
            .table
            .list(&[], "created_at DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list",
                serde_json::json!({"limit": limit}),
                error,
            );
        }
        result
    }

    pub async fn get(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let result = self
            .table
            .get(&[Where("id", SqlVal::Str(task_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    pub async fn update(
        &self,
        task_id: &str,
        name: Option<&str>,
        cron_expr: Option<&str>,
        prompt: Option<&str>,
        enabled: Option<bool>,
    ) -> Result<()> {
        let mut builder = sql::Sql::update("tasks")
            .set_opt("name", name)
            .set_opt("cron_expr", cron_expr)
            .set_opt("prompt", prompt);
        if let Some(e) = enabled {
            builder = builder.set_raw("enabled", if e { "true" } else { "false" });
        }
        builder = builder
            .set_raw("updated_at", "NOW()")
            .where_eq("id", task_id);
        if !builder.has_sets() {
            return Ok(());
        }
        let result = self.table.pool().exec(&builder.build()).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "update",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, task_id: &str) -> Result<()> {
        let sql_str = format!(
            "DELETE FROM tasks WHERE id = '{}'",
            sql::escape(task_id)
        );
        let result = self.table.pool().exec(&sql_str).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    pub async fn toggle(&self, task_id: &str) -> Result<()> {
        let sql_str = format!(
            "UPDATE tasks SET enabled = NOT enabled, updated_at = NOW() WHERE id = '{}'",
            sql::escape(task_id)
        );
        let result = self.table.pool().exec(&sql_str).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "toggle",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }
}
