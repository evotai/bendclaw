use anyhow::Result;
use bendclaw::storage::sql::col_i64;
use bendclaw::storage::versioned::delete_versioned;
use bendclaw::storage::versioned::gen_id;
use bendclaw::storage::versioned::insert_versioned;
use bendclaw::storage::versioned::update_versioned;
use bendclaw_test_harness::setup::pool;
use bendclaw_test_harness::setup::uid;

// ── gen_id ──

#[test]
fn gen_id_returns_nonempty_string() {
    let id = gen_id();
    assert!(!id.is_empty());
}

#[test]
fn gen_id_returns_unique_values() {
    let a = gen_id();
    let b = gen_id();
    assert_ne!(a, b);
}

#[tokio::test]
async fn versioned_update_and_delete_increment_version_with_single_sql_path() -> Result<()> {
    let pool = pool().await?;
    let table = format!("test_versioned_{}", uid("tbl").replace('-', "_"));

    pool.exec(&format!(
        "CREATE TABLE {table} (\
         id VARCHAR NOT NULL, \
         version INT NOT NULL, \
         action VARCHAR NOT NULL, \
         payload VARCHAR NOT NULL DEFAULT '', \
         created_at TIMESTAMP NOT NULL DEFAULT NOW()\
         )"
    ))
    .await?;

    let id = uid("item");

    insert_versioned(&pool, &table, &id, "payload", "'v1'").await?;
    update_versioned(&pool, &table, &id, "payload", "'v2'").await?;
    delete_versioned(&pool, &table, &id).await?;

    let rows = pool
        .query_all(&format!(
            "SELECT version, action FROM {table} \
             WHERE id = '{}' ORDER BY version ASC",
            bendclaw::storage::sql::escape(&id)
        ))
        .await?;

    assert_eq!(rows.len(), 3);
    assert_eq!(col_i64(&rows[0], 0), 1);
    assert_eq!(bendclaw::storage::sql::col(&rows[0], 1), "create");
    assert_eq!(col_i64(&rows[1], 0), 2);
    assert_eq!(bendclaw::storage::sql::col(&rows[1], 1), "update");
    assert_eq!(col_i64(&rows[2], 0), 3);
    assert_eq!(bendclaw::storage::sql::col(&rows[2], 1), "delete");

    Ok(())
}
