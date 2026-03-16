use anyhow::Context as _;
use anyhow::Result;
use bendclaw::storage::dal::knowledge::build_search_condition;
use bendclaw::storage::dal::knowledge::KnowledgeRecord;
use bendclaw::storage::dal::knowledge::KnowledgeRepo;
use bendclaw::storage::Pool;

use crate::common::setup::require_api_config;
use crate::common::setup::uid;

const RECALL_MIGRATION: &str = include_str!("../../../migrations/base/recall.sql");

async fn setup_pool() -> Result<Option<Pool>> {
    let (base_url, token, warehouse) = match require_api_config() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("skipping query_fts test: {e}");
            return Ok(None);
        }
    };
    if token.is_empty() {
        eprintln!("skipping query_fts test: missing token");
        return Ok(None);
    }
    let root = Pool::new(&base_url, &token, &warehouse)?;
    // Verify connectivity before proceeding.
    if root.query_all("SELECT 1").await.is_err() {
        eprintln!("skipping query_fts test: Databend not reachable");
        return Ok(None);
    }
    let db_name = format!("test_query_fts_{}", &uid("t")[..8]);
    root.exec(&format!("CREATE DATABASE IF NOT EXISTS `{db_name}`"))
        .await?;
    let pool = root.with_database(&db_name)?;
    run_migration(&pool, RECALL_MIGRATION).await?;
    Ok(Some(pool))
}

async fn run_migration(pool: &Pool, sql: &str) -> Result<()> {
    for stmt in sql.split(';') {
        let stmt = stmt.trim();
        let has_code = stmt
            .lines()
            .any(|l| !l.trim().is_empty() && !l.trim().starts_with("--"));
        if !has_code {
            continue;
        }
        pool.exec(stmt)
            .await
            .with_context(|| format!("migration failed:\n{stmt}"))?;
    }
    Ok(())
}

fn make_record(id: &str, subject: &str, summary: &str, locator: &str) -> KnowledgeRecord {
    KnowledgeRecord {
        id: id.into(),
        kind: "discovery".into(),
        subject: subject.into(),
        locator: locator.into(),
        title: "test".into(),
        summary: summary.into(),
        metadata: None,
        status: "active".into(),
        confidence: 1.0,
        user_id: "u1".into(),
        first_run_id: "r1".into(),
        last_run_id: "r1".into(),
        first_seen_at: String::new(),
        last_seen_at: String::new(),
        created_at: String::new(),
        updated_at: String::new(),
    }
}

/// Basic multi-column QUERY() search works and returns results.
#[tokio::test]
async fn query_fts_multi_column_search() -> Result<()> {
    let Some(pool) = setup_pool().await? else {
        return Ok(());
    };
    let repo = KnowledgeRepo::new(pool);

    let id = uid("k");
    repo.insert(&make_record(
        &id,
        "rust_compiler",
        "How to compile Rust programs",
        "/usr/bin/rustc",
    ))
    .await?;

    // Search should match via summary
    let results = repo.search("compile Rust", 10).await?;
    assert!(
        results.iter().any(|r| r.id == id),
        "expected to find record by summary match"
    );

    // Search should match via subject
    let results = repo.search("rust_compiler", 10).await?;
    assert!(
        results.iter().any(|r| r.id == id),
        "expected to find record by subject match"
    );

    // Search should match via locator
    let results = repo.search("rustc", 10).await?;
    assert!(
        results.iter().any(|r| r.id == id),
        "expected to find record by locator match"
    );

    Ok(())
}

/// QUERY() does not crash on special characters in user input.
#[tokio::test]
async fn query_fts_special_chars_no_error() -> Result<()> {
    let Some(pool) = setup_pool().await? else {
        return Ok(());
    };
    let repo = KnowledgeRepo::new(pool);

    let cases = vec![
        "c++",
        "file:path",
        "it's a test",
        "(hello world)",
        "[1 TO 5]",
        "a*b?c",
        "term^2",
        "~fuzzy",
        "a+b-c",
        r#"say "hi""#,
        r"back\slash",
    ];

    for input in cases {
        let result = repo.search(input, 5).await;
        assert!(
            result.is_ok(),
            "QUERY() failed on input {:?}: {:?}",
            input,
            result.err()
        );
    }

    Ok(())
}

/// Verify build_search_condition produces valid SQL that Databend accepts.
#[tokio::test]
async fn query_fts_raw_condition_accepted_by_databend() -> Result<()> {
    let Some(pool) = setup_pool().await? else {
        return Ok(());
    };

    let cond = build_search_condition("bendclaw readme");
    let sql = format!(
        "SELECT id FROM knowledge WHERE {} ORDER BY SCORE() DESC LIMIT 5",
        cond
    );
    let result = pool.query_all(&sql).await;
    assert!(
        result.is_ok(),
        "Databend rejected generated SQL: {:?}\nSQL: {}",
        result.err(),
        sql
    );

    Ok(())
}
