use bendclaw::storage::Pool;

// ── Unit tests (no network) ──────────────────────────────────────────────────

#[test]
fn pool_new_valid() {
    let pool = Pool::new("https://app.databend.com", "test-token", "default");
    assert!(pool.is_ok());
}

#[test]
fn pool_base_url_accessor() {
    let pool = Pool::new("https://app.databend.com", "test-token", "default").unwrap();
    assert_eq!(pool.base_url(), "https://app.databend.com/v1");
}

#[test]
fn pool_base_url_trims_trailing_slash() {
    let pool = Pool::new("https://app.databend.com/", "test-token", "default").unwrap();
    assert_eq!(pool.base_url(), "https://app.databend.com/v1");
}

#[test]
fn pool_base_url_preserves_version() {
    let pool = Pool::new("https://app.databend.com/v1.1", "test-token", "default").unwrap();
    assert_eq!(pool.base_url(), "https://app.databend.com/v1.1");
}

#[test]
fn pool_with_database() {
    let pool = Pool::new("https://app.databend.com", "test-token", "default").unwrap();
    let new_pool = pool.with_database("testdb").unwrap();
    assert_eq!(new_pool.base_url(), "https://app.databend.com/v1");
}

#[test]
fn pool_debug_hides_token() {
    let pool = Pool::new("https://app.databend.com", "secret-token-123", "default").unwrap();
    let debug = format!("{:?}", pool);
    assert!(debug.contains("***"));
    assert!(!debug.contains("secret-token-123"));
}

// ── Integration tests (require Databend Cloud credentials) ───────────────────

fn require_pool() -> Option<Pool> {
    let (base_url, token, warehouse) = crate::common::setup::require_api_config().ok()?;
    if token.is_empty() {
        return None;
    }
    Pool::new(&base_url, &token, &warehouse).ok()
}

#[tokio::test]
async fn http_select_one() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let rows = pool.query_all("SELECT 1 AS n").await.unwrap();
    assert_eq!(rows.len(), 1);
}

#[tokio::test]
async fn http_exec_and_query() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let db = format!(
        "test_pool_it_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );

    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await
        .unwrap();
    let db_pool = pool.with_database(&db).unwrap();

    db_pool
        .exec("CREATE TABLE t_pool_test (id INT, name VARCHAR)")
        .await
        .unwrap();
    db_pool
        .exec("INSERT INTO t_pool_test VALUES (1, 'alice'), (2, 'bob')")
        .await
        .unwrap();

    let rows = db_pool
        .query_all("SELECT id, name FROM t_pool_test ORDER BY id")
        .await
        .unwrap();
    assert_eq!(rows.len(), 2);

    let row0 = rows[0].as_array().unwrap();
    assert_eq!(row0[0].as_str().unwrap(), "1");
    assert_eq!(row0[1].as_str().unwrap(), "alice");

    let row1 = rows[1].as_array().unwrap();
    assert_eq!(row1[0].as_str().unwrap(), "2");
    assert_eq!(row1[1].as_str().unwrap(), "bob");

    // cleanup
    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await
        .unwrap();
}

#[tokio::test]
async fn http_query_row_returns_first() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let row = pool
        .query_row("SELECT 42 AS answer")
        .await
        .unwrap()
        .unwrap();
    let arr = row.as_array().unwrap();
    assert_eq!(arr[0].as_str().unwrap(), "42");
}

#[tokio::test]
async fn http_query_row_empty_returns_none() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let db = format!(
        "test_pool_empty_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await
        .unwrap();
    let db_pool = pool.with_database(&db).unwrap();
    db_pool.exec("CREATE TABLE t_empty (id INT)").await.unwrap();

    let row = db_pool.query_row("SELECT id FROM t_empty").await.unwrap();
    assert!(row.is_none());

    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await
        .unwrap();
}

#[tokio::test]
async fn http_with_database_switches_context() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let db = format!(
        "test_pool_ctx_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await
        .unwrap();
    let db_pool = pool.with_database(&db).unwrap();

    let row = db_pool
        .query_row("SELECT currentDatabase()")
        .await
        .unwrap()
        .unwrap();
    let arr = row.as_array().unwrap();
    assert_eq!(arr[0].as_str().unwrap(), db);

    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await
        .unwrap();
}

#[tokio::test]
async fn http_invalid_sql_returns_error() {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return;
    };
    let result = pool.exec("INVALID SQL STATEMENT").await;
    assert!(result.is_err());
}
