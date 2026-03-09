use super::record::VariableRecord;
use crate::base::Result;
use crate::storage::dal::logging::repo_error;
use crate::storage::pool::Pool;
use crate::storage::sql;
use crate::storage::sql::SqlVal;
use crate::storage::table::DatabendTable;
use crate::storage::table::RowMapper;
use crate::storage::table::Where;

const REPO: &str = "variable";

#[derive(Clone)]
struct VariableMapper;

impl RowMapper for VariableMapper {
    type Entity = VariableRecord;

    fn columns(&self) -> &str {
        "id, key, value, secret, TO_VARCHAR(created_at), TO_VARCHAR(updated_at)"
    }

    fn parse(&self, row: &serde_json::Value) -> VariableRecord {
        let secret_str: String = sql::col(row, 3);
        let secret = matches!(secret_str.as_str(), "1" | "true");
        VariableRecord {
            id: sql::col(row, 0),
            key: sql::col(row, 1),
            value: sql::col(row, 2),
            secret,
            created_at: sql::col(row, 4),
            updated_at: sql::col(row, 5),
        }
    }
}

#[derive(Clone)]
pub struct VariableRepo {
    table: DatabendTable<VariableMapper>,
}

impl VariableRepo {
    pub fn new(pool: Pool) -> Self {
        Self {
            table: DatabendTable::new(pool, "variables", VariableMapper),
        }
    }

    pub async fn insert(&self, record: &VariableRecord) -> Result<()> {
        let secret_val = if record.secret { "true" } else { "false" };
        let result = self
            .table
            .insert(&[
                ("id", SqlVal::Str(&record.id)),
                ("key", SqlVal::Str(&record.key)),
                ("value", SqlVal::Str(&record.value)),
                ("secret", SqlVal::Raw(secret_val)),
                ("created_at", SqlVal::Raw("NOW()")),
                ("updated_at", SqlVal::Raw("NOW()")),
            ])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "insert",
                serde_json::json!({"variable_id": record.id}),
                error,
            );
        }
        result
    }

    pub async fn list(&self, limit: u32) -> Result<Vec<VariableRecord>> {
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

    pub async fn get(&self, id: &str) -> Result<Option<VariableRecord>> {
        let result = self
            .table
            .get(&[Where("id", SqlVal::Str(id))])
            .await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "get",
                serde_json::json!({"variable_id": id}),
                error,
            );
        }
        result
    }

    pub async fn update(
        &self,
        id: &str,
        key: &str,
        value: &str,
        secret: bool,
    ) -> Result<()> {
        let key_e = sql::escape(key);
        let value_e = sql::escape(value);
        let id_e = sql::escape(id);
        let sql = format!(
            "UPDATE variables SET key='{}', value='{}', secret={}, updated_at=NOW() WHERE id='{}'",
            key_e, value_e, secret, id_e
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "update",
                serde_json::json!({"variable_id": id}),
                error,
            );
        }
        result
    }

    pub async fn delete(&self, id: &str) -> Result<()> {
        let sql = format!(
            "DELETE FROM variables WHERE id = '{}'",
            sql::escape(id)
        );
        let result = self.table.pool().exec(&sql).await;
        if let Err(error) = &result {
            repo_error(
                REPO,
                "delete",
                serde_json::json!({"variable_id": id}),
                error,
            );
        }
        result
    }
}