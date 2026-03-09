use super::record::LearningRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "learnings";

#[derive(Clone)]
struct LearningMapper;

impl RowMapper for LearningMapper {
    type Entity = LearningRecord;

    fn columns(&self) -> &str {
        "id, agent_id, user_id, session_id, title, content, tags, source, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> LearningRecord {
        LearningRecord {
            id: sql::col(row, 0),
            agent_id: sql::col(row, 1),
            user_id: sql::col(row, 2),
            session_id: sql::col(row, 3),
            title: sql::col(row, 4),
            content: sql::col(row, 5),
            tags: sql::col(row, 6),
            source: sql::col(row, 7),
            created_at: sql::col(row, 8),
            updated_at: sql::col(row, 9),
        }
    }
}

#[derive(Clone)]
pub struct LearningRepo {
    table: DatabendTable<LearningMapper>,
}

impl LearningRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "learnings", LearningMapper),
        }
    }

    pub async fn insert(&self, record: &LearningRecord) -> Result<()> {
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("agent_id", SqlVal::Str(&record.agent_id)),
                ("user_id", SqlVal::Str(&record.user_id)),
                ("session_id", SqlVal::Str(&record.session_id)),
                ("title", SqlVal::Str(&record.title)),
                ("content", SqlVal::Str(&record.content)),
                ("tags", SqlVal::Str(&record.tags)),
                ("source", SqlVal::Str(&record.source)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
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

    pub async fn list_by_agent(&self, agent_id: &str, limit: u32) -> Result<Vec<LearningRecord>> {
        let result = self
            .table
            .list(
                &[Where("agent_id", SqlVal::Str(agent_id))],
                "created_at DESC",
                limit as u64,
            )
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "list_by_agent",
                serde_json::json!({"agent_id": agent_id, "limit": limit}),
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

    pub async fn delete(&self, learning_id: &str) -> Result<()> {
        let sql = format!(
            "DELETE FROM learnings WHERE id = '{}'",
            sql::escape(learning_id)
        );
        let result = self.table.pool().exec(&sql).await;
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

    pub async fn search(
        &self,
        agent_id: &str,
        query: &str,
        limit: u32,
    ) -> Result<Vec<LearningRecord>> {
        let aid = sql::escape(agent_id);
        let q = sql::escape(query);
        let cond = format!("agent_id = '{aid}' AND MATCH(content, '{q}')");
        let result = self
            .table
            .list_where(&cond, "SCORE() DESC", limit as u64)
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "search",
                serde_json::json!({"agent_id": agent_id, "query": query}),
                error,
            );
        }
        result
    }
}
