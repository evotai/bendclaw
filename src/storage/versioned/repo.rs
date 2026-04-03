use crate::storage::pool::Pool;
use crate::storage::sql::escape;
use crate::storage::time::now;
use crate::types::Result;

/// Insert a new versioned record (action = 'create').
pub async fn insert_versioned(
    pool: &Pool,
    table: &str,
    id: &str,
    columns: &str,
    values: &str,
) -> Result<()> {
    let sql = format!(
        "INSERT INTO {table} (id, version, action, {columns}, created_at) \
         VALUES ('{}', 1, 'create', {values}, '{}')",
        escape(id),
        now().format("%Y-%m-%d %H:%M:%S")
    );
    pool.exec(&sql).await
}

/// Update a versioned record (action = 'update').
/// Uses a single INSERT ... SELECT statement to derive the next version.
pub async fn update_versioned(
    pool: &Pool,
    table: &str,
    id: &str,
    columns: &str,
    values: &str,
) -> Result<()> {
    let escaped_id = escape(id);
    let sql = format!(
        "INSERT INTO {table} (id, version, action, {columns}, created_at) \
         SELECT '{escaped_id}', COALESCE(MAX(version), 0) + 1, 'update', {values}, '{}' \
         FROM {table} WHERE id = '{escaped_id}'",
        now().format("%Y-%m-%d %H:%M:%S")
    );
    pool.exec(&sql).await
}

/// Soft-delete a versioned record (action = 'delete').
/// Uses a single INSERT ... SELECT statement to derive the next version.
pub async fn delete_versioned(pool: &Pool, table: &str, id: &str) -> Result<()> {
    let escaped_id = escape(id);
    let sql = format!(
        "INSERT INTO {table} (id, version, action, created_at) \
         SELECT '{escaped_id}', COALESCE(MAX(version), 0) + 1, 'delete', '{}' \
         FROM {table} WHERE id = '{escaped_id}'",
        now().format("%Y-%m-%d %H:%M:%S")
    );
    pool.exec(&sql).await
}
