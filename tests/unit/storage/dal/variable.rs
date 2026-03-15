use anyhow::Result;
use bendclaw::storage::VariableRepo;

use crate::common::fake_databend::paged_rows;
use crate::common::fake_databend::FakeDatabend;
use crate::common::fake_databend::FakeDatabendCall;

#[tokio::test]
async fn variable_repo_list_active_filters_revoked_and_parses_fields() -> Result<()> {
    let fake = FakeDatabend::new(|sql, database| {
        assert_eq!(database, None);
        assert_eq!(
            sql,
            "SELECT id, key, value, secret, revoked, TO_VARCHAR(last_used_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM variables WHERE revoked = FALSE ORDER BY created_at DESC LIMIT 5"
        );
        Ok(paged_rows(
            &[&[
                "var-1",
                "API_TOKEN",
                "secret-value",
                "true",
                "false",
                "2026-03-11T00:00:00Z",
                "2026-03-10T00:00:00Z",
                "2026-03-11T00:00:00Z",
            ]],
            None,
            None,
        ))
    });
    let repo = VariableRepo::new(fake.pool());

    let variables = repo.list_active(5).await?;

    assert_eq!(variables.len(), 1);
    assert_eq!(variables[0].id, "var-1");
    assert!(variables[0].secret);
    assert!(!variables[0].revoked);
    assert_eq!(
        variables[0].last_used_at.as_deref(),
        Some("2026-03-11T00:00:00Z")
    );
    Ok(())
}

#[tokio::test]
async fn variable_repo_get_builds_id_lookup_query() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "SELECT id, key, value, secret, revoked, TO_VARCHAR(last_used_at), TO_VARCHAR(created_at), TO_VARCHAR(updated_at) FROM variables WHERE id = 'var-7' LIMIT 1"
        );
        Ok(paged_rows(
            &[&[
                "var-7",
                "LOG_LEVEL",
                "debug",
                "false",
                "false",
                "",
                "2026-03-10T00:00:00Z",
                "2026-03-10T00:00:00Z",
            ]],
            None,
            None,
        ))
    });
    let repo = VariableRepo::new(fake.pool());

    let variable = repo.get("var-7").await?.expect("variable should exist");

    assert_eq!(variable.key, "LOG_LEVEL");
    assert_eq!(variable.value, "debug");
    assert!(!variable.secret);
    Ok(())
}

#[tokio::test]
async fn variable_repo_touch_last_used_many_updates_each_id() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert_eq!(
            sql,
            "UPDATE variables SET last_used_at=NOW() WHERE id IN ('var-1', 'var-2')"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = VariableRepo::new(fake.pool());

    repo.touch_last_used_many(&["var-1".to_string(), "var-2".to_string()])
        .await?;

    assert_eq!(fake.calls(), vec![FakeDatabendCall::Query {
        sql: "UPDATE variables SET last_used_at=NOW() WHERE id IN ('var-1', 'var-2')".to_string(),
        database: None,
    },]);
    Ok(())
}

#[tokio::test]
async fn variable_repo_touch_last_used_many_empty_ids_is_noop() -> Result<()> {
    let fake = FakeDatabend::new(|_sql, _database| {
        panic!("no SQL should be issued for empty ids");
    });
    let repo = VariableRepo::new(fake.pool());

    repo.touch_last_used_many(&[]).await?;

    assert!(fake.calls().is_empty());
    Ok(())
}

#[tokio::test]
async fn variable_repo_list_all_uses_max_limit() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql.contains("LIMIT 10000"),
            "list_all must use MAX_LIST_LIMIT: {sql}"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = VariableRepo::new(fake.pool());

    let variables = repo.list_all().await?;
    assert!(variables.is_empty());
    Ok(())
}

#[tokio::test]
async fn variable_repo_list_all_active_uses_max_limit() -> Result<()> {
    let fake = FakeDatabend::new(|sql, _database| {
        assert!(
            sql.contains("revoked = FALSE"),
            "list_all_active must filter revoked: {sql}"
        );
        assert!(
            sql.contains("LIMIT 10000"),
            "list_all_active must use MAX_LIST_LIMIT: {sql}"
        );
        Ok(paged_rows(&[], None, None))
    });
    let repo = VariableRepo::new(fake.pool());

    let variables = repo.list_all_active().await?;
    assert!(variables.is_empty());
    Ok(())
}
