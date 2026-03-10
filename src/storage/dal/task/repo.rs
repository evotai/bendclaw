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
        "id, executor_instance_id, name, cron_expr, prompt, enabled, status, schedule_kind, every_seconds, TO_VARCHAR(at_time), tz, webhook_url, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> TaskRecord {
        let enabled_str = sql::col(row, 5);
        let enabled = enabled_str == "1" || enabled_str.eq_ignore_ascii_case("true");
        let delete_after_run_str = sql::col(row, 13);
        let delete_after_run =
            delete_after_run_str == "1" || delete_after_run_str.eq_ignore_ascii_case("true");
        TaskRecord {
            id: sql::col(row, 0),
            executor_instance_id: sql::col(row, 1),
            name: sql::col(row, 2),
            cron_expr: sql::col(row, 3),
            prompt: sql::col(row, 4),
            enabled,
            status: sql::col(row, 6),
            schedule_kind: sql::col(row, 7),
            every_seconds: sql::col_opt(row, 8).and_then(|s| s.parse().ok()),
            at_time: sql::col_opt(row, 9),
            tz: sql::col_opt(row, 10),
            webhook_url: sql::col_opt(row, 11),
            last_error: sql::col_opt(row, 12),
            delete_after_run,
            run_count: sql::col_i32(row, 14),
            last_run_at: sql::col(row, 15),
            next_run_at: sql::col_opt(row, 16),
            lease_token: sql::col_opt(row, 17),
            created_at: sql::col(row, 18),
            updated_at: sql::col(row, 19),
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
        let delete_after_run_raw = if record.delete_after_run {
            "true"
        } else {
            "false"
        };
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                (
                    "executor_instance_id",
                    SqlVal::Str(&record.executor_instance_id),
                ),
                ("name", SqlVal::Str(&record.name)),
                ("cron_expr", SqlVal::Str(&record.cron_expr)),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("enabled", SqlVal::Raw(enabled_raw)),
                ("status", SqlVal::Str(&record.status)),
                ("schedule_kind", SqlVal::Str(&record.schedule_kind)),
                ("every_seconds", match record.every_seconds {
                    Some(v) => SqlVal::Int(v as i64),
                    None => SqlVal::Null,
                }),
                ("at_time", SqlVal::str_or_null(record.at_time.as_deref())),
                ("tz", SqlVal::str_or_null(record.tz.as_deref())),
                (
                    "webhook_url",
                    SqlVal::str_or_null(record.webhook_url.as_deref()),
                ),
                ("last_error", SqlVal::Null),
                ("delete_after_run", SqlVal::Raw(delete_after_run_raw)),
                ("run_count", SqlVal::Int(0)),
                ("last_run_at", SqlVal::Raw("NULL")),
                (
                    "next_run_at",
                    SqlVal::str_or_null(record.next_run_at.as_deref()),
                ),
                ("lease_token", SqlVal::Null),
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
        let result = self.table.list(&[], "created_at DESC", limit as u64).await;
        if let Err(error) = &result {
            repo_error(REPO, "list", serde_json::json!({"limit": limit}), error);
        }
        result
    }

    pub async fn get(&self, task_id: &str) -> Result<Option<TaskRecord>> {
        let result = self.table.get(&[Where("id", SqlVal::Str(task_id))]).await;
        if let Err(error) = &result {
            repo_error(REPO, "get", serde_json::json!({"task_id": task_id}), error);
        }
        result
    }

    /// List tasks that are due for execution (enabled and next_run_at <= NOW()).
    /// Only returns tasks whose executor_instance_id matches the given instance.
    pub async fn list_due(&self, executor_instance_id: &str) -> Result<Vec<TaskRecord>> {
        let condition = format!(
            "enabled = true AND status != 'running' AND next_run_at <= NOW() AND executor_instance_id = '{}'",
            crate::storage::sql::escape(executor_instance_id)
        );
        let result = self
            .table
            .list_where(&condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_due",
                serde_json::json!({"executor_instance_id": executor_instance_id}),
                error,
            );
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        &self,
        task_id: &str,
        name: Option<&str>,
        cron_expr: Option<&str>,
        prompt: Option<&str>,
        enabled: Option<bool>,
        schedule_kind: Option<&str>,
        every_seconds: Option<Option<i32>>,
        at_time: Option<Option<&str>>,
        tz: Option<Option<&str>>,
        webhook_url: Option<Option<&str>>,
        delete_after_run: Option<bool>,
        next_run_at: Option<Option<&str>>,
    ) -> Result<()> {
        let mut builder = sql::Sql::update("tasks")
            .set_opt("name", name)
            .set_opt("cron_expr", cron_expr)
            .set_opt("prompt", prompt)
            .set_opt("schedule_kind", schedule_kind);
        if let Some(e) = enabled {
            builder = builder.set_raw("enabled", if e { "true" } else { "false" });
        }
        if let Some(v) = every_seconds {
            match v {
                Some(secs) => builder = builder.set("every_seconds", secs),
                None => builder = builder.set_raw("every_seconds", "NULL"),
            }
        }
        if let Some(v) = at_time {
            match v {
                Some(t) => builder = builder.set("at_time", t),
                None => builder = builder.set_raw("at_time", "NULL"),
            }
        }
        if let Some(v) = tz {
            match v {
                Some(t) => builder = builder.set("tz", t),
                None => builder = builder.set_raw("tz", "NULL"),
            }
        }
        if let Some(v) = webhook_url {
            match v {
                Some(u) => builder = builder.set("webhook_url", u),
                None => builder = builder.set_raw("webhook_url", "NULL"),
            }
        }
        if let Some(d) = delete_after_run {
            builder = builder.set_raw("delete_after_run", if d { "true" } else { "false" });
        }
        if let Some(v) = next_run_at {
            match v {
                Some(t) => builder = builder.set("next_run_at", t),
                None => builder = builder.set_raw("next_run_at", "NULL"),
            }
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

    /// Update task status and scheduling fields after execution.
    pub async fn update_after_run(
        &self,
        task_id: &str,
        status: &str,
        last_error: Option<&str>,
        next_run_at: Option<&str>,
    ) -> Result<()> {
        let mut builder = sql::Sql::update("tasks")
            .set("status", status)
            .set_raw("last_run_at", "NOW()")
            .set_raw("run_count", "run_count + 1")
            .set_raw("updated_at", "NOW()");
        match last_error {
            Some(e) => builder = builder.set("last_error", e),
            None => builder = builder.set_raw("last_error", "NULL"),
        }
        match next_run_at {
            Some(t) => builder = builder.set("next_run_at", t),
            None => builder = builder.set_raw("next_run_at", "NULL"),
        }
        builder = builder.where_eq("id", task_id);
        let result = self.table.pool().exec(&builder.build()).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "update_after_run",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    /// Atomically claim due tasks by setting status='running' and assigning a lease_token.
    /// Returns the claimed tasks.
    pub async fn claim_due_tasks(
        &self,
        executor_instance_id: &str,
        lease_token: &str,
    ) -> Result<Vec<TaskRecord>> {
        let update_sql = sql::Sql::update("tasks")
            .set("status", "running")
            .set("lease_token", lease_token)
            .set_raw("updated_at", "NOW()")
            .where_raw("enabled = true")
            .where_raw("status != 'running'")
            .where_raw("next_run_at <= NOW()")
            .where_eq("executor_instance_id", executor_instance_id)
            .build();
        if let Err(e) = self.table.pool().exec(&update_sql).await {
            repo_error(
                REPO,
                "claim_due_tasks",
                serde_json::json!({"executor_instance_id": executor_instance_id}),
                &e,
            );
            return Err(e);
        }
        // Fetch the claimed tasks
        let condition = format!(
            "lease_token = '{}' AND status = 'running'",
            sql::escape(lease_token)
        );
        let result = self
            .table
            .list_where(&condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "claim_due_tasks:select",
                serde_json::json!({"lease_token": lease_token}),
                error,
            );
        }
        result
    }

    /// Update task after execution with lease_token verification.
    pub async fn finish_task(
        &self,
        task_id: &str,
        lease_token: &str,
        status: &str,
        last_error: Option<&str>,
        next_run_at: Option<&str>,
    ) -> Result<()> {
        let mut builder = sql::Sql::update("tasks")
            .set("status", status)
            .set_raw("lease_token", "NULL")
            .set_raw("last_run_at", "NOW()")
            .set_raw("run_count", "run_count + 1")
            .set_raw("updated_at", "NOW()");
        match last_error {
            Some(e) => builder = builder.set("last_error", e),
            None => builder = builder.set_raw("last_error", "NULL"),
        }
        match next_run_at {
            Some(t) => builder = builder.set("next_run_at", t),
            None => builder = builder.set_raw("next_run_at", "NULL"),
        }
        builder = builder
            .where_eq("id", task_id)
            .where_eq("lease_token", lease_token);
        let result = self.table.pool().exec(&builder.build()).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "finish_task",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, task_id: &str) -> Result<()> {
        let sql_str = format!("DELETE FROM tasks WHERE id = '{}'", sql::escape(task_id));
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
