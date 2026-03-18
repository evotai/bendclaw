use std::time::Duration;

use super::record::LearningRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::Sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "learnings";
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct LearningMapper;

impl RowMapper for LearningMapper {
    type Entity = LearningRecord;

    fn columns(&self) -> &str {
        "id, kind, subject, title, content, conditions, strategy, \
         priority, confidence, status, supersedes_id, user_id, source_run_id, \
         success_count, failure_count, TO_VARCHAR(last_applied_at), \
         TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<LearningRecord> {
        let conditions_raw = sql::col(row, 5);
        let conditions = if conditions_raw.is_empty() || conditions_raw == "NULL" {
            None
        } else {
            serde_json::from_str(&conditions_raw).ok()
        };
        let strategy_raw = sql::col(row, 6);
        let strategy = if strategy_raw.is_empty() || strategy_raw == "NULL" {
            None
        } else {
            serde_json::from_str(&strategy_raw).ok()
        };
        let priority = sql::col_i32(row, 7).unwrap_or(0);
        let confidence = sql::col_f64(row, 8).unwrap_or(1.0);
        let success_count = sql::col_i32(row, 13).unwrap_or(0);
        let failure_count = sql::col_i32(row, 14).unwrap_or(0);
        let last_applied = sql::col(row, 15);
        Ok(LearningRecord {
            id: sql::col(row, 0),
            kind: sql::col(row, 1),
            subject: sql::col(row, 2),
            title: sql::col(row, 3),
            content: sql::col(row, 4),
            conditions,
            strategy,
            priority,
            confidence,
            status: sql::col(row, 9),
            supersedes_id: sql::col(row, 10),
            user_id: sql::col(row, 11),
            source_run_id: sql::col(row, 12),
            success_count,
            failure_count,
            last_applied_at: if last_applied.is_empty() {
                None
            } else {
                Some(last_applied)
            },
            created_at: sql::col(row, 16),
            updated_at: sql::col(row, 17),
        })
    }
}

#[derive(Clone)]
pub struct LearningRepo {
    table: DatabendTable<LearningMapper>,
}

impl LearningRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "learnings", LearningMapper).with_ttl_cache(CACHE_TTL),
        }
    }

    pub async fn insert(&self, record: &LearningRecord) -> Result<()> {
        let priority_str = record.priority.to_string();
        let confidence_str = record.confidence.to_string();
        let conditions_json = record
            .conditions
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let strategy_json = record
            .strategy
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let mut cols: Vec<(&str, SqlVal)> = vec![
            ("id", SqlVal::Str(&record.id)),
            ("kind", SqlVal::Str(&record.kind)),
            ("subject", SqlVal::Str(&record.subject)),
            ("title", SqlVal::Str(&record.title)),
            ("content", SqlVal::Str(&record.content)),
            ("priority", SqlVal::Raw(&priority_str)),
            ("confidence", SqlVal::Raw(&confidence_str)),
            ("status", SqlVal::Str(&record.status)),
            ("supersedes_id", SqlVal::Str(&record.supersedes_id)),
            ("user_id", SqlVal::Str(&record.user_id)),
            ("source_run_id", SqlVal::Str(&record.source_run_id)),
            ("created_at", SqlVal::Raw("NOW()")),
            ("updated_at", SqlVal::Raw("NOW()")),
        ];
        if let Some(ref cj) = conditions_json {
            cols.push(("conditions", SqlVal::Str(cj)));
        }
        if let Some(ref sj) = strategy_json {
            cols.push(("strategy", SqlVal::Str(sj)));
        }
        let result = self.table.insert(&cols).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"learning_id": record.id}),
                error,
            );
        }
        result
    }

    pub async fn get(&self, learning_id: &str) -> Result<Option<LearningRecord>> {
        let result = self
            .table
            .get(&[Where("id", SqlVal::Str(learning_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get",
                serde_json::json!({"learning_id": learning_id}),
                error,
            );
        }
        result
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<LearningRecord>> {
        let result = self
            .table
            .list(&[], "priority DESC, updated_at DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "list", serde_json::json!({"limit": limit}), error);
        }
        result
    }

    pub async fn list_active_by_kind(&self, kind: &str, limit: u32) -> Result<Vec<LearningRecord>> {
        let k = sql::escape(kind);
        let cond = format!("status = 'active' AND kind = '{k}'");
        let result = self
            .table
            .list_where(&cond, "priority DESC, updated_at DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_active_by_kind",
                serde_json::json!({"kind": kind, "limit": limit}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, learning_id: &str) -> Result<()> {
        let sql = format!(
            "DELETE FROM learnings WHERE id = '{}'",
            sql::escape(learning_id)
        );
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete",
                serde_json::json!({"learning_id": learning_id}),
                error,
            );
        }
        result
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<LearningRecord>> {
        let q = sql::escape_query(query);
        let cond = format!("QUERY('content:{q}')");
        let result = self
            .table
            .list_where(&cond, "SCORE() DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "search", serde_json::json!({"query": query}), error);
        }
        result
    }

    pub async fn increment_success(&self, id: &str) -> Result<()> {
        let sql = Sql::update("learnings")
            .set_raw("success_count", "success_count + 1")
            .set_raw("last_applied_at", "NOW()")
            .set_raw("updated_at", "NOW()")
            .where_eq("id", id)
            .build();
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "increment_success",
                serde_json::json!({"id": id}),
                error,
            );
        }
        result
    }

    pub async fn increment_failure(&self, id: &str) -> Result<()> {
        let sql = Sql::update("learnings")
            .set_raw("failure_count", "failure_count + 1")
            .set_raw("last_applied_at", "NOW()")
            .set_raw("updated_at", "NOW()")
            .where_eq("id", id)
            .build();
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "increment_failure",
                serde_json::json!({"id": id}),
                error,
            );
        }
        result
    }
}
