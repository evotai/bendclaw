use super::record::RunKind;
use super::record::RunRecord;
use super::record::RunStatus;
use crate::storage::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

const REPO: &str = "runs";

#[derive(Clone)]
struct RunMapper;

impl RowMapper for RunMapper {
    type Entity = RunRecord;

    fn columns(&self) -> &str {
        "id, session_id, agent_id, user_id, kind, parent_run_id, node_id, status, input, output, error, metrics, stop_reason, checkpoint_through_run_id, iterations, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<RunRecord> {
        Ok(RunRecord {
            id: sql::col(row, 0),
            session_id: sql::col(row, 1),
            agent_id: sql::col(row, 2),
            user_id: sql::col(row, 3),
            kind: sql::col(row, 4),
            parent_run_id: sql::col(row, 5),
            node_id: sql::col(row, 6),
            status: sql::col(row, 7),
            input: sql::col(row, 8),
            output: sql::col(row, 9),
            error: sql::col(row, 10),
            metrics: sql::col(row, 11),
            stop_reason: sql::col(row, 12),
            checkpoint_through_run_id: sql::col(row, 13),
            iterations: sql::col_u32(row, 14)?,
            created_at: sql::col(row, 15),
            updated_at: sql::col(row, 16),
        })
    }
}

#[derive(Clone)]
pub struct RunRepo {
    table: DatabendTable<RunMapper>,
}

impl RunRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "runs", RunMapper),
        }
    }

    pub async fn insert(&self, record: &RunRecord) -> Result<()> {
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("session_id", SqlVal::Str(&record.session_id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("user_id", SqlVal::Str(&record.user_id)),
                ("kind", SqlVal::Str(&record.kind)),
                ("parent_run_id", SqlVal::Str(&record.parent_run_id)),
                ("node_id", SqlVal::Str(&record.node_id)),
                ("status", SqlVal::Str(&record.status)),
                ("input", SqlVal::Str(&record.input)),
                ("output", SqlVal::Str(&record.output)),
                ("error", SqlVal::Str(&record.error)),
                ("metrics", SqlVal::Str(&record.metrics)),
                ("stop_reason", SqlVal::Str(&record.stop_reason)),
                (
                    "checkpoint_through_run_id",
                    SqlVal::Str(&record.checkpoint_through_run_id),
                ),
                ("iterations", SqlVal::Raw(&record.iterations.to_string())),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"run_id": record.id}),
                error,
            );
        }
        result
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update_final(
        &self,
        run_id: &str,
        status: RunStatus,
        output: &str,
        error: &str,
        metrics: &str,
        stop_reason: &str,
        iterations: u32,
    ) -> Result<()> {
        let sql = format!(
            "UPDATE runs SET status = '{}', output = '{}', error = '{}', metrics = '{}', stop_reason = '{}', iterations = {}, updated_at = NOW() WHERE id = '{}'",
            status.as_str(),
            sql::escape(output),
            sql::escape(error),
            sql::escape(metrics),
            sql::escape(stop_reason),
            iterations,
            sql::escape(run_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(err) = &result {
            repo_error(
                REPO,
                "update_final",
                serde_json::json!({"run_id": run_id, "status": status.as_str()}),
                err,
            );
        }
        result
    }

    pub async fn update_status(&self, run_id: &str, status: RunStatus) -> Result<()> {
        let sql = format!(
            "UPDATE runs SET status = '{}', updated_at = NOW() WHERE id = '{}'",
            status.as_str(),
            sql::escape(run_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "update_status",
                serde_json::json!({"run_id": run_id, "status": status.as_str()}),
                error,
            );
        }
        result
    }

    pub async fn load(&self, run_id: &str) -> Result<Option<RunRecord>> {
        let result = self.table.get(&[Where("id", SqlVal::Str(run_id))]).await;
        if let Err(error) = &result {
            repo_error(REPO, "load", serde_json::json!({"run_id": run_id}), error);
        }
        result
    }

    pub async fn count_for_session(&self, session_id: &str, status: Option<&str>) -> Result<u64> {
        let condition = visible_session_condition(session_id, status);
        let result = async {
            let query = format!("SELECT COUNT(*) FROM runs WHERE {condition}");
            let row = self.table.pool().query_row(&query).await?;
            sql::agg_u64_or_zero(row.as_ref(), 0)
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "count_for_session",
                serde_json::json!({"session_id": session_id, "status": status}),
                error,
            );
        }
        result
    }

    pub async fn list_for_session(
        &self,
        session_id: &str,
        status: Option<&str>,
        order: &str,
        limit: u64,
        offset: u64,
    ) -> Result<Vec<RunRecord>> {
        let condition = visible_session_condition(session_id, status);
        let result = async {
            let query = format!(
                "SELECT {} FROM runs WHERE {condition} ORDER BY created_at {order} LIMIT {limit} OFFSET {offset}",
                RunMapper.columns()
            );
            let rows = self.table.pool().query_all(&query).await?;
            rows.iter().map(|row| RunMapper.parse(row)).collect()
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_for_session",
                serde_json::json!({
                    "session_id": session_id,
                    "status": status,
                    "order": order,
                    "limit": limit,
                    "offset": offset
                }),
                error,
            );
        }
        result
    }

    pub async fn list_by_session(&self, session_id: &str, limit: u32) -> Result<Vec<RunRecord>> {
        let condition = visible_session_condition(session_id, None);
        let result = self
            .table
            .list_where(&condition, "created_at DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_session",
                serde_json::json!({"session_id": session_id, "limit": limit}),
                error,
            );
        }
        result
    }

    pub async fn load_latest_checkpoint(&self, session_id: &str) -> Result<Option<RunRecord>> {
        let sid = sql::escape(session_id);
        let condition = format!(
            "session_id = '{sid}' AND kind = '{}'",
            RunKind::SessionCheckpoint.as_str()
        );
        let result = async {
            let mut rows = self
                .table
                .list_where(&condition, "created_at DESC", 1)
                .await?;
            Ok(rows.pop())
        }
        .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "load_latest_checkpoint",
                serde_json::json!({"session_id": session_id}),
                error,
            );
        }
        result
    }
}

fn visible_session_condition(session_id: &str, status: Option<&str>) -> String {
    let mut condition = format!(
        "session_id = '{}' AND kind != '{}'",
        sql::escape(session_id),
        RunKind::SessionCheckpoint.as_str()
    );
    if let Some(status) = status.filter(|status| !status.is_empty()) {
        condition.push_str(&format!(" AND status = '{}'", sql::escape(status)));
    }
    condition
}
