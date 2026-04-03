use super::record::RunEventRecord;
use crate::storage::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;
use crate::types::Result;

const REPO: &str = "run_events";

#[derive(Clone)]
struct RunEventMapper;

impl RowMapper for RunEventMapper {
    type Entity = RunEventRecord;

    fn columns(&self) -> &str {
        "id, run_id, session_id, agent_id, user_id, seq, event, payload, TO_VARCHAR(created_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::types::Result<RunEventRecord> {
        Ok(RunEventRecord {
            id: sql::col(row, 0),
            run_id: sql::col(row, 1),
            session_id: sql::col(row, 2),
            agent_id: sql::col(row, 3),
            user_id: sql::col(row, 4),
            seq: sql::col_u32(row, 5)?,
            event: sql::col(row, 6),
            payload: sql::col(row, 7),
            created_at: sql::col(row, 8),
        })
    }
}

#[derive(Clone)]
pub struct RunEventRepo {
    table: DatabendTable<RunEventMapper>,
}

impl RunEventRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "run_events", RunEventMapper),
        }
    }

    pub async fn insert_batch(&self, records: &[RunEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let columns = &[
            "id",
            "run_id",
            "session_id",
            "agent_id",
            "user_id",
            "seq",
            "event",
            "payload",
            "created_at",
        ];

        let mut seq_values: Vec<String> = Vec::with_capacity(records.len());
        for record in records {
            seq_values.push(record.seq.to_string());
        }

        let mut rows: Vec<Vec<SqlVal<'_>>> = Vec::with_capacity(records.len());
        for (idx, record) in records.iter().enumerate() {
            rows.push(vec![
                SqlVal::Str(&record.id),
                SqlVal::Str(&record.run_id),
                SqlVal::Str(&record.session_id),
                SqlVal::Str(&record.agent_id),
                SqlVal::Str(&record.user_id),
                SqlVal::Raw(&seq_values[idx]),
                SqlVal::Str(&record.event),
                SqlVal::Str(&record.payload),
                SqlVal::Raw("NOW()"),
            ]);
        }

        let result = self.table.insert_batch(columns, &rows).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert_batch",
                serde_json::json!({"count": records.len()}),
                error,
            );
        }
        result
    }

    pub async fn list_by_run(&self, run_id: &str, limit: u32) -> Result<Vec<RunEventRecord>> {
        let result = self
            .table
            .list(
                &[Where("run_id", SqlVal::Str(run_id))],
                "seq ASC, created_at ASC",
                limit as u64,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_run",
                serde_json::json!({"run_id": run_id, "limit": limit}),
                error,
            );
        }
        result
    }

    /// Load replay-relevant events for multiple runs in a single query.
    /// Only returns event types consumed by the replay projector:
    /// ToolStart, ToolEnd, and sem.capabilities_snapshot.
    /// Ordered by created_at then seq for correct cross-run chronology.
    pub async fn list_by_runs(&self, run_ids: &[&str], limit: u32) -> Result<Vec<RunEventRecord>> {
        if run_ids.is_empty() {
            return Ok(Vec::new());
        }
        let in_list: String = run_ids
            .iter()
            .map(|id| format!("'{}'", sql::escape(id)))
            .collect::<Vec<_>>()
            .join(", ");
        let condition = format!(
            "run_id IN ({in_list}) AND event IN ('ToolStart', 'ToolEnd', 'sem.capabilities_snapshot')"
        );
        let result = self
            .table
            .list_where(&condition, "created_at ASC, seq ASC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_runs",
                serde_json::json!({"run_count": run_ids.len(), "limit": limit}),
                error,
            );
        }
        result
    }
}
