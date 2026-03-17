use super::delivery::TaskDelivery;
use super::record::TaskRecord;
use super::schedule::TaskSchedule;
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
        "id, executor_node_id, name, prompt, enabled, status, schedule, delivery, last_error, delete_after_run, run_count, TO_VARCHAR(last_run_at), TO_VARCHAR(next_run_at), lease_token, lease_node_id, TO_VARCHAR(lease_expires_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<TaskRecord> {
        let enabled_str = sql::col(row, 4);
        let enabled = enabled_str == "1" || enabled_str.eq_ignore_ascii_case("true");
        let delete_after_run_str = sql::col(row, 9);
        let delete_after_run =
            delete_after_run_str == "1" || delete_after_run_str.eq_ignore_ascii_case("true");
        Ok(TaskRecord {
            id: sql::col(row, 0),
            executor_node_id: sql::col(row, 1),
            name: sql::col(row, 2),
            prompt: sql::col(row, 3),
            enabled,
            status: sql::col(row, 5),
            schedule: TaskSchedule::from_storage(&sql::col(row, 6), "tasks.schedule")?,
            delivery: TaskDelivery::from_storage(&sql::col(row, 7), "tasks.delivery")?,
            last_error: sql::col_opt(row, 8),
            delete_after_run,
            run_count: sql::col_i32(row, 10)?,
            last_run_at: sql::col(row, 11),
            next_run_at: sql::col_opt(row, 12),
            lease_token: sql::col_opt(row, 13),
            lease_node_id: sql::col_opt(row, 14),
            lease_expires_at: sql::col_opt(row, 15),
            created_at: sql::col(row, 16),
            updated_at: sql::col(row, 17),
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
                ("executor_node_id", SqlVal::Str(&record.executor_node_id)),
                ("name", SqlVal::Str(&record.name)),
                ("prompt", SqlVal::Str(&record.prompt)),
                ("enabled", SqlVal::Raw(enabled_raw)),
                ("status", SqlVal::Str(&record.status)),
                ("schedule", SqlVal::Raw(&schedule_expr)),
                ("delivery", SqlVal::Raw(&delivery_expr)),
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

    /// List tasks that are due for execution (enabled and next_run_at <= NOW()).
    /// Only returns tasks whose executor_node_id matches the given instance.
    pub async fn list_due(&self, executor_node_id: &str) -> Result<Vec<TaskRecord>> {
        let condition = format!(
            "enabled = true AND status != 'running' AND next_run_at <= NOW() AND executor_node_id = '{}'",
            crate::storage::sql::escape(executor_node_id)
        );
        let result = self
            .table
            .list_where(&condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_due",
                serde_json::json!({"executor_node_id": executor_node_id}),
                error,
            );
        }
        result
    }

    /// List all due tasks regardless of executor_node_id (for lease-based scheduling).
    /// Also picks up tasks stuck in 'running' with expired leases (crash recovery).
    pub async fn list_due_any(&self) -> Result<Vec<TaskRecord>> {
        let condition = "enabled = true AND next_run_at <= NOW() AND (\
            status != 'running' \
            OR (status = 'running' AND (lease_expires_at IS NULL OR lease_expires_at <= NOW()))\
        )";
        let result = self
            .table
            .list_where(condition, "next_run_at ASC", 100)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "list_due_any", serde_json::json!({}), error);
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

    /// Atomically claim due tasks by setting status='running' and assigning a lease_token.
    /// Returns the claimed tasks.
    pub async fn claim_due_tasks(
        &self,
        executor_node_id: &str,
        lease_token: &str,
    ) -> Result<Vec<TaskRecord>> {
        let update_sql = sql::Sql::update("tasks")
            .set("status", "running")
            .set("lease_token", lease_token)
            .set_raw("updated_at", "NOW()")
            .where_raw("enabled = true")
            .where_raw("status != 'running'")
            .where_raw("(lease_token IS NULL OR lease_token = '')")
            .where_raw("next_run_at <= NOW()")
            .where_eq("executor_node_id", executor_node_id)
            .build();
        if let Err(e) = self.table.pool().exec(&update_sql).await {
            repo_error(
                REPO,
                "claim_due_tasks",
                serde_json::json!({"executor_node_id": executor_node_id}),
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
}
