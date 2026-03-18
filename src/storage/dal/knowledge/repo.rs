use std::time::Duration;

use super::record::KnowledgeRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "knowledge";
const CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct KnowledgeMapper;

impl RowMapper for KnowledgeMapper {
    type Entity = KnowledgeRecord;

    fn columns(&self) -> &str {
        "id, kind, subject, locator, title, summary, metadata, status, confidence, \
         user_id, first_run_id, last_run_id, \
         TO_VARCHAR(first_seen_at), TO_VARCHAR(last_seen_at), \
         TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> crate::base::Result<KnowledgeRecord> {
        let meta_raw = sql::col(row, 6);
        let metadata = if meta_raw.is_empty() || meta_raw == "NULL" {
            None
        } else {
            serde_json::from_str(&meta_raw).ok()
        };
        let confidence = sql::col_f64(row, 8).unwrap_or(1.0);
        Ok(KnowledgeRecord {
            id: sql::col(row, 0),
            kind: sql::col(row, 1),
            subject: sql::col(row, 2),
            locator: sql::col(row, 3),
            title: sql::col(row, 4),
            summary: sql::col(row, 5),
            metadata,
            status: sql::col(row, 7),
            confidence,
            user_id: sql::col(row, 9),
            first_run_id: sql::col(row, 10),
            last_run_id: sql::col(row, 11),
            first_seen_at: sql::col(row, 12),
            last_seen_at: sql::col(row, 13),
            created_at: sql::col(row, 14),
            updated_at: sql::col(row, 15),
        })
    }
}

#[derive(Clone)]
pub struct KnowledgeRepo {
    table: DatabendTable<KnowledgeMapper>,
}

impl KnowledgeRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "knowledge", KnowledgeMapper).with_ttl_cache(CACHE_TTL),
        }
    }

    pub async fn insert(&self, record: &KnowledgeRecord) -> Result<()> {
        let meta_json = record
            .metadata
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default());
        let confidence_str = record.confidence.to_string();
        let mut cols: Vec<(&str, SqlVal)> = vec![
            ("id", SqlVal::Str(&record.id)),
            ("kind", SqlVal::Str(&record.kind)),
            ("subject", SqlVal::Str(&record.subject)),
            ("locator", SqlVal::Str(&record.locator)),
            ("title", SqlVal::Str(&record.title)),
            ("summary", SqlVal::Str(&record.summary)),
            ("status", SqlVal::Str(&record.status)),
            ("confidence", SqlVal::Raw(&confidence_str)),
            ("user_id", SqlVal::Str(&record.user_id)),
            ("first_run_id", SqlVal::Str(&record.first_run_id)),
            ("last_run_id", SqlVal::Str(&record.last_run_id)),
            ("first_seen_at", SqlVal::Raw("NOW()")),
            ("last_seen_at", SqlVal::Raw("NOW()")),
            ("created_at", SqlVal::Raw("NOW()")),
            ("updated_at", SqlVal::Raw("NOW()")),
        ];
        if let Some(ref mj) = meta_json {
            cols.push(("metadata", SqlVal::Str(mj)));
        }
        let result = self.table.insert(&cols).await;
        if let Err(error) = &result {
            repo_error(REPO, "insert", serde_json::json!({"id": record.id}), error);
        }
        result
    }

    pub async fn get(&self, id: &str) -> Result<Option<KnowledgeRecord>> {
        let result = self.table.get(&[Where("id", SqlVal::Str(id))]).await;
        if let Err(error) = &result {
            repo_error(REPO, "get", serde_json::json!({"id": id}), error);
        }
        result
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<KnowledgeRecord>> {
        let result = self.table.list(&[], "updated_at DESC", limit as u64).await;
        if let Err(error) = &result {
            repo_error(REPO, "list", serde_json::json!({"limit": limit}), error);
        }
        result
    }

    pub async fn list_active(&self, limit: u32) -> Result<Vec<KnowledgeRecord>> {
        let result = self
            .table
            .list_where("status = 'active'", "updated_at DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_active",
                serde_json::json!({"limit": limit}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let sql = format!("DELETE FROM knowledge WHERE id = '{}'", sql::escape(id));
        let result = self.table.exec_raw(&sql).await;
        if let Err(error) = &result {
            repo_error(REPO, "delete", serde_json::json!({"id": id}), error);
        }
        result
    }

    pub async fn search(&self, query: &str, limit: u32) -> Result<Vec<KnowledgeRecord>> {
        let cond = build_search_condition(query);
        let result = self
            .table
            .list_where(&cond, "SCORE() DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(REPO, "search", serde_json::json!({"query": query}), error);
        }
        result
    }
}

/// Build the WHERE condition for knowledge full-text search using QUERY().
/// Searches across subject (highest boost), summary, and locator columns.
pub fn build_search_condition(query: &str) -> String {
    let q = sql::escape_query(query);
    format!("QUERY('subject:{q}^3 OR summary:{q}^2 OR locator:{q}')")
}
