use anyhow::Context as _;
use anyhow::Result;
use bendclaw::storage::Pool;

// ── Unit tests (no network) ──────────────────────────────────────────────────

#[test]
fn pool_new_valid() {
    let pool = Pool::new("https://app.databend.com", "test-token", "default");
    assert!(pool.is_ok());
}

#[test]
fn pool_base_url_accessor() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "test-token", "default")?;
    assert_eq!(pool.base_url(), "https://app.databend.com/v1");
    Ok(())
}

#[test]
fn pool_base_url_trims_trailing_slash() -> Result<()> {
    let pool = Pool::new("https://app.databend.com/", "test-token", "default")?;
    assert_eq!(pool.base_url(), "https://app.databend.com/v1");
    Ok(())
}

#[test]
fn pool_base_url_preserves_version() -> Result<()> {
    let pool = Pool::new("https://app.databend.com/v1.1", "test-token", "default")?;
    assert_eq!(pool.base_url(), "https://app.databend.com/v1.1");
    Ok(())
}

#[test]
fn pool_with_database() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "test-token", "default")?;
    let new_pool = pool.with_database("testdb")?;
    assert_eq!(new_pool.base_url(), "https://app.databend.com/v1");
    Ok(())
}

#[test]
fn pool_debug_hides_token() -> Result<()> {
    let pool = Pool::new("https://app.databend.com", "secret-token-123", "default")?;
    let debug = format!("{:?}", pool);
    assert!(debug.contains("***"));
    assert!(!debug.contains("secret-token-123"));
    Ok(())
}

// ── Integration tests (require Databend Cloud credentials) ───────────────────

fn require_pool() -> Option<Pool> {
    let (base_url, token, warehouse) = bendclaw_test_harness::setup::require_api_config().ok()?;
    if token.is_empty() {
        return None;
    }
    Pool::new(&base_url, &token, &warehouse).ok()
}

#[tokio::test]
async fn http_select_one() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let rows = pool.query_all("SELECT 1 AS n").await?;
    assert_eq!(rows.len(), 1);
    Ok(())
}

#[tokio::test]
async fn http_exec_and_query() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let db = format!(
        "test_pool_it_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );

    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await?;
    let db_pool = pool.with_database(&db)?;

    db_pool
        .exec("CREATE TABLE t_pool_test (id INT, name VARCHAR)")
        .await?;
    db_pool
        .exec("INSERT INTO t_pool_test VALUES (1, 'alice'), (2, 'bob')")
        .await?;

    let rows = db_pool
        .query_all("SELECT id, name FROM t_pool_test ORDER BY id")
        .await?;
    assert_eq!(rows.len(), 2);

    let row0 = rows[0].as_array().context("expected array row 0")?;
    assert_eq!(row0[0].as_str().context("expected str at [0][0]")?, "1");
    assert_eq!(row0[1].as_str().context("expected str at [0][1]")?, "alice");

    let row1 = rows[1].as_array().context("expected array row 1")?;
    assert_eq!(row1[0].as_str().context("expected str at [1][0]")?, "2");
    assert_eq!(row1[1].as_str().context("expected str at [1][1]")?, "bob");

    // cleanup
    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await?;
    Ok(())
}

#[tokio::test]
async fn http_query_row_returns_first() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let row = pool
        .query_row("SELECT 42 AS answer")
        .await?
        .context("expected a row")?;
    let arr = row.as_array().context("expected array")?;
    assert_eq!(arr[0].as_str().context("expected str")?, "42");
    Ok(())
}

#[tokio::test]
async fn http_query_row_empty_returns_none() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let db = format!(
        "test_pool_empty_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await?;
    let db_pool = pool.with_database(&db)?;
    db_pool.exec("CREATE TABLE t_empty (id INT)").await?;

    let row = db_pool.query_row("SELECT id FROM t_empty").await?;
    assert!(row.is_none());

    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await?;
    Ok(())
}

#[tokio::test]
async fn http_with_database_switches_context() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let db = format!(
        "test_pool_ctx_{}",
        &ulid::Ulid::new().to_string().to_lowercase()[..8]
    );
    pool.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db}`"))
        .await?;
    let db_pool = pool.with_database(&db)?;

    let row = db_pool
        .query_row("SELECT currentDatabase()")
        .await?
        .context("expected a row")?;
    let arr = row.as_array().context("expected array")?;
    assert_eq!(arr[0].as_str().context("expected str")?, db);

    pool.exec(&format!("DROP DATABASE IF EXISTS `{db}`"))
        .await?;
    Ok(())
}

#[tokio::test]
async fn http_invalid_sql_returns_error() -> Result<()> {
    let Some(pool) = require_pool() else {
        eprintln!("skipping: no Databend Cloud credentials");
        return Ok(());
    };
    let result = pool.exec("INVALID SQL STATEMENT").await;
    assert!(result.is_err());
    Ok(())
}
