use anyhow::Result;
use bendclaw::storage::Pool;

use crate::common::fake_databend::api_error;
use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

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
    assert!(!debug.contains("secret-token-123"));
    assert!(debug.contains("base_url"));
    assert!(debug.contains("warehouse"));
    Ok(())
}

#[tokio::test]
async fn pool_query_all_uses_paging_and_finalize_on_injected_client() -> Result<()> {
    let fake = FakeDatabend::with_handlers(
        |sql, database| {
            assert_eq!(sql, "SELECT value FROM demo");
            assert_eq!(database, Some("testdb"));
            Ok(paged_rows(
                &[&["first"]],
                Some("/v1/query/page-2"),
                Some("/v1/query/final"),
            ))
        },
        |uri| {
            assert_eq!(uri, "/v1/query/page-2");
            Ok(paged_rows(&[&["second"]], None, None))
        },
        |uri| {
            assert_eq!(uri, "/v1/query/final");
            Ok(())
        },
    );
    let pool = fake.pool().with_database("testdb")?;

    let rows = pool.query_all("SELECT value FROM demo").await?;

    assert_eq!(rows, vec![
        serde_json::json!(["first"]),
        serde_json::json!(["second"]),
    ]);
    assert_eq!(fake.calls(), vec![
        FakeDatabendCall::Query {
            sql: "SELECT value FROM demo".to_string(),
            database: Some("testdb".to_string()),
        },
        FakeDatabendCall::Page {
            uri: "/v1/query/page-2".to_string(),
        },
        FakeDatabendCall::Finalize {
            uri: "/v1/query/final".to_string(),
        },
    ]);
    Ok(())
}

#[tokio::test]
async fn pool_exec_classifies_api_error_from_injected_client() {
    let fake =
        FakeDatabend::new(|_sql, _database| Ok(api_error(3901, "Unknown database 'missing_db'")));
    let pool = fake.pool();

    let error = pool.exec("SELECT 1").await.expect_err("query should fail");

    assert_eq!(error.code, bendclaw::types::ErrorCode::NOT_FOUND);
    assert!(error.message.contains("Unknown database"));
}

// ── Pool::noop ──────────────────────────────────────────────────────────────

#[test]
fn pool_noop_creates_successfully() {
    let pool = Pool::noop();
    // normalize_base_url appends /v1
    assert!(pool.base_url().starts_with("noop:"));
}

#[tokio::test]
async fn pool_noop_query_returns_error() {
    let pool = Pool::noop();
    let err = pool.exec("SELECT 1").await;
    assert!(err.is_err(), "noop pool should return error on query");
}
