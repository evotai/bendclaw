use super::record::FeedbackRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "feedback";

#[derive(Clone)]
struct FeedbackMapper;

impl RowMapper for FeedbackMapper {
    type Entity = FeedbackRecord;

    fn columns(&self) -> &str {
        "id, session_id, run_id, rating, comment, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> FeedbackRecord {
        FeedbackRecord {
            id: sql::col(row, 0),
            session_id: sql::col(row, 1),
            run_id: sql::col(row, 2),
            rating: sql::col_i32(row, 3),
            comment: sql::col(row, 4),
            created_at: sql::col(row, 5),
            updated_at: sql::col(row, 6),
        }
    }
}

#[derive(Clone)]
pub struct FeedbackRepo {
    table: DatabendTable<FeedbackMapper>,
}

impl FeedbackRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "feedback", FeedbackMapper),
        }
    }

    pub async fn insert(&self, record: &FeedbackRecord) -> Result<()> {
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("session_id", SqlVal::Str(&record.session_id)),
                ("run_id", SqlVal::Str(&record.run_id)),
                ("rating", SqlVal::Int(record.rating as i64)),
                ("comment", SqlVal::Str(&record.comment)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"feedback_id": record.id}),
                error,
            );
        }
        result
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<FeedbackRecord>> {
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

    pub async fn get(&self, feedback_id: &str) -> Result<Option<FeedbackRecord>> {
        let result = self
            .table
            .get(&[Where("id", SqlVal::Str(feedback_id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get",
                serde_json::json!({"feedback_id": feedback_id}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, feedback_id: &str) -> Result<()> {
        let sql = format!(
            "DELETE FROM feedback WHERE id = '{}'",
            sql::escape(feedback_id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete",
                serde_json::json!({"feedback_id": feedback_id}),
                error,
            );
        }
        result
    }
}