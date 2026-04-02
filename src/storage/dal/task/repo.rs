use super::delivery::TaskDelivery;
use super::record::TaskRecord;
use super::schedule::TaskSchedule;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

const REPO: &str = "task";

#[derive(Clone)]
struct TaskMapper;

impl RowMapper for TaskMapper {
    type Entity = TaskRecord;

    fn columns(&self) -> &str {
        "id, node_id, name, prompt, enabled, status, schedule, delivery, user_id, scope, created_by, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, lease_node_id, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<TaskRecord> {
        let enabled_str = sql::col(row, 4);
        let enabled = enabled_str == "1" || enabled_str.eq_ignore_ascii_case("true");
        let delete_after_run_str: String = sql::col(row, 12);
        let delete_after_run =
            delete_after_run_str == "1" || delete_after_run_str.eq_ignore_ascii_case("true");
        Ok(TaskRecord {
            id: sql::col(row, 0),
            node_id: sql::col(row, 1),
            name: sql::col(row, 2),
            prompt: sql::col(row, 3),
            enabled,
            status: sql::col(row, 5),
            schedule: TaskSchedule::from_storage(&sql::col(row, 6), "tasks.schedule")?,
            delivery: TaskDelivery::from_storage(&sql::col(row, 7), "tasks.delivery")?,
            user_id: sql::col(row, 8),
            scope: sql::col(row, 9),
            created_by: sql::col(row, 10),
            last_error: sql::col_opt(row, 11),
            delete_after_run,
            run_count: sql::col_i32(row, 13)?,
            last_run_at: sql::col(row, 14),
            next_run_at: sql::col_opt(row, 15),
            lease_token: sql::col_opt(row, 16),
            lease_node_id: sql::col_opt(row, 17),
            lease_expires_at: sql::col_opt(row, 18),
            created_at: sql::col(row, 19),
            updated_at: sql::col(row, 20),
        })
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
        let schedule_expr = record.schedule.to_storage_expr()?;
        let delivery_expr = record.delivery.to_storage_expr()?;
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("node_id", SqlVal::Str(&record.node_id)),
                ("name", SqlVal::Str(&record.name)),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("enabled", SqlVal::Raw(enabled_raw)),
                ("status", SqlVal::Str(&record.status)),
                ("schedule", SqlVal::Raw(&schedule_expr)),
                ("delivery", SqlVal::Raw(&delivery_expr)),
                ("user_id", SqlVal::Str(&record.user_id)),
                ("scope", SqlVal::Str(&record.scope)),
                ("created_by", SqlVal::Str(&record.created_by)),
                ("last_error", SqlVal::Null),
                ("delete_after_run", SqlVal::Raw(delete_after_run_raw)),
                ("run_count", SqlVal::Int(0)),
                ("last_run_at", SqlVal::Raw("NULL")),
                (
                    "next_run_at",
                    SqlVal::str_or_null(record.next_run_at.as_deref()),
                ),
                ("lease_token", SqlVal::Null),
                ("lease_node_id", SqlVal::Null),
                ("lease_expires_at", SqlVal::Raw("NULL")),
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

    /// List all due tasks (for lease-based scheduling).
    /// Also picks up tasks stuck in 'running' with expired leases (crash recovery).
    pub async fn list_due(&self) -> Result<Vec<TaskRecord>> {
        let condition = "enabled = true AND next_run_at <= NOW() AND (\
            status != 'running' \
            OR (status = 'running' AND (lease_expires_at IS NULL OR lease_expires_at <= NOW()))\
        )";
        let result = self
            .table
            .list_where(condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "list_due", serde_json::json!({}), error);
        }
        result
    }

    /// List all tasks that are currently active: due and claimable, OR running
    /// with a valid lease. Used by discover() to prevent stale eviction of
    /// held tasks that are still executing.
    /// Note: running tasks are included regardless of `enabled` — disabling a
    /// task mid-execution must not cause stale eviction (which would lose results).
    pub async fn list_active(&self) -> Result<Vec<TaskRecord>> {
        let condition = "(\
            (enabled = true AND next_run_at <= NOW() AND (\
                status != 'running' \
                OR (status = 'running' AND (lease_expires_at IS NULL OR lease_expires_at <= NOW()))\
            )) \
            OR (status = 'running' AND lease_token IS NOT NULL AND lease_token != '')\
        )";
        let result = self
            .table
            .list_where(condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "list_active", serde_json::json!({}), error);
        }
        result
    }

    /// Set task status to 'running' (called by task domain layer on acquisition).
    pub async fn set_status_running(&self, task_id: &str) -> Result<()> {
        let sql_str = sql::Sql::update("tasks")
            .set("status", "running")
            .set_raw("updated_at", "NOW()")
            .where_eq("id", task_id)
            .where_raw("status != 'running'")
            .build();
        let result = self.table.pool().exec(&sql_str).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "set_status_running",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }

    /// Reset task status from 'running' to 'idle' (abnormal exit recovery).
    /// No-op if finish_task already set a final status.
    pub async fn reset_status_if_running(&self, task_id: &str) -> Result<()> {
        let sql_str = sql::Sql::update("tasks")
            .set("status", "idle")
            .set_raw("updated_at", "NOW()")
            .where_eq("id", task_id)
            .where_raw("status = 'running'")
            .build();
        let result = self.table.pool().exec(&sql_str).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "reset_status_if_running",
                serde_json::json!({"task_id": task_id}),
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
        prompt: Option<&str>,
        enabled: Option<bool>,
        schedule: Option<&TaskSchedule>,
        delivery: Option<&TaskDelivery>,
        delete_after_run: Option<bool>,
        next_run_at: Option<Option<&str>>,
    ) -> Result<()> {
        let mut builder = sql::Sql::update("tasks")
            .set_opt("name", name)
            .set_opt("prompt", prompt);
        if let Some(e) = enabled {
            builder = builder.set_raw("enabled", if e { "true" } else { "false" });
        }
        if let Some(v) = schedule {
            let expr = v.to_storage_expr()?;
            builder = builder.set_raw("schedule", &expr);
        }
        if let Some(v) = delivery {
            let expr = v.to_storage_expr()?;
            builder = builder.set_raw("delivery", &expr);
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
            .set_raw("lease_node_id", "NULL")
            .set_raw("lease_expires_at", "NULL")
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

    /// Set next_run_at to NOW() so the scheduler picks up the task immediately.
    pub async fn trigger_now(&self, task_id: &str) -> Result<()> {
        let sql_str = format!(
            "UPDATE tasks SET next_run_at = NOW(), enabled = true, status = 'idle', updated_at = NOW() WHERE id = '{}'",
            sql::escape(task_id)
        );
        let result = self.table.pool().exec(&sql_str).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "trigger_now",
                serde_json::json!({"task_id": task_id}),
                error,
            );
        }
        result
    }
}
