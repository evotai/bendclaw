//! Tests for Variables (agent/variables.rs).

use std::sync::Arc;

use bend_engine::tools::GetVariableResponse;
use bendclaw::agent::Variables;
use bendclaw::storage::fs::FsStorage;
use bendclaw::storage::Storage;
use bendclaw::types::VariableRecord;
use bendclaw::types::VariableScope;

type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

async fn make_variables(dir: &std::path::Path) -> Arc<Variables> {
    let storage: Arc<dyn Storage> = Arc::new(FsStorage::new(dir.to_path_buf()));
    Arc::new(Variables::new(storage, Vec::new()))
}

async fn make_variables_with_storage(dir: &std::path::Path) -> (Arc<Variables>, Arc<dyn Storage>) {
    let storage: Arc<dyn Storage> = Arc::new(FsStorage::new(dir.to_path_buf()));
    let vars = Arc::new(Variables::new(storage.clone(), Vec::new()));
    (vars, storage)
}

fn expect_found(resp: GetVariableResponse) -> Result<String> {
    match resp {
        GetVariableResponse::Found(v) => Ok(v),
        GetVariableResponse::NotFound => Err("expected Found, got NotFound".into()),
    }
}

// ---------------------------------------------------------------------------
// set / list / delete
// ---------------------------------------------------------------------------

#[tokio::test]
async fn set_and_list_global() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("API_KEY".into(), "abc".into()).await?;
    vars.set_global("DB_HOST".into(), "localhost".into())
        .await?;

    let items = vars.list_global();
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert!(keys.contains(&"API_KEY"));
    assert!(keys.contains(&"DB_HOST"));
    assert_eq!(items.len(), 2);
    Ok(())
}

#[tokio::test]
async fn set_overwrites_existing() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("KEY".into(), "old".into()).await?;
    vars.set_global("KEY".into(), "new".into()).await?;

    let items = vars.list_global();
    assert_eq!(items.len(), 1);
    Ok(())
}

#[tokio::test]
async fn delete_existing_key() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("KEY".into(), "val".into()).await?;
    let removed = vars.delete_global("KEY").await?;
    assert!(removed);
    assert!(vars.list_global().is_empty());
    Ok(())
}

#[tokio::test]
async fn delete_nonexistent_key() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    let removed = vars.delete_global("NOPE").await?;
    assert!(!removed);
    Ok(())
}

#[tokio::test]
async fn has_variables() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    assert!(!vars.has_variables());
    vars.set_global("K".into(), "V".into()).await?;
    assert!(vars.has_variables());
    Ok(())
}

// ---------------------------------------------------------------------------
// import via set_global (simulating REPL flow)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn import_env_via_set_global() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    let pairs = vec![
        ("API_KEY".to_string(), "abc123".to_string()),
        ("DB_HOST".to_string(), "localhost".to_string()),
        ("QUOTED".to_string(), "hello world".to_string()),
    ];
    for (key, value) in pairs {
        vars.set_global(key, value).await?;
    }

    let keys: Vec<String> = vars.list_global().iter().map(|i| i.key.clone()).collect();
    assert!(keys.contains(&"API_KEY".to_string()));
    assert!(keys.contains(&"DB_HOST".to_string()));
    assert!(keys.contains(&"QUOTED".to_string()));
    assert_eq!(keys.len(), 3);
    Ok(())
}

// ---------------------------------------------------------------------------
// get_for_context — scope resolution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn get_for_context_global_found() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("TOKEN".into(), "secret".into()).await?;

    let resp = vars
        .get_for_context("TOKEN", "/some/cwd", "sess_1")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(expect_found(resp)?, "secret");
    Ok(())
}

#[tokio::test]
async fn get_for_context_not_found() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    let resp = vars
        .get_for_context("MISSING", "/cwd", "sess_1")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert!(matches!(resp, GetVariableResponse::NotFound));
    Ok(())
}

#[tokio::test]
async fn get_for_context_updates_last_used() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("KEY".into(), "val".into()).await?;

    let items = vars.list_global();
    assert!(items[0].last_used_at.is_none());
    assert!(items[0].last_used_by.is_none());
    assert_eq!(items[0].used_count, 0);

    let _ = vars
        .get_for_context("KEY", "/cwd", "sess_abc")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let items = vars.list_global();
    assert!(items[0].last_used_at.is_some());
    assert_eq!(items[0].last_used_by.as_deref(), Some("sess_abc"));
    assert_eq!(items[0].used_count, 1);
    Ok(())
}

#[tokio::test]
async fn list_global_sorts_by_used_count_then_last_used_desc() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("BETA".into(), "2".into()).await?;
    vars.set_global("ALPHA".into(), "1".into()).await?;
    vars.set_global("GAMMA".into(), "3".into()).await?;

    let _ = vars
        .get_for_context("BETA", "/cwd", "sess_1")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let _ = vars
        .get_for_context("BETA", "/cwd", "sess_2")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    let _ = vars
        .get_for_context("ALPHA", "/cwd", "sess_3")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

    let items = vars.list_global();
    let keys: Vec<&str> = items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, vec!["BETA", "ALPHA", "GAMMA"]);
    assert_eq!(items[0].used_count, 2);
    assert_eq!(items[1].used_count, 1);
    assert_eq!(items[2].used_count, 0);
    Ok(())
}

#[tokio::test]
async fn as_get_fn_returns_value() -> Result {
    let tmp = tempfile::tempdir()?;
    let vars = make_variables(tmp.path()).await;

    vars.set_global("MY_VAR".into(), "hello".into()).await?;

    let get_fn = vars.as_get_fn("/cwd", "sess_1");
    let resp = get_fn("MY_VAR".into())
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(expect_found(resp)?, "hello");
    Ok(())
}

// ---------------------------------------------------------------------------
// persistence roundtrip
// ---------------------------------------------------------------------------

#[tokio::test]
async fn persistence_roundtrip() -> Result {
    let tmp = tempfile::tempdir()?;
    let (vars, storage) = make_variables_with_storage(tmp.path()).await;

    vars.set_global("A".into(), "1".into()).await?;
    vars.set_global("B".into(), "2".into()).await?;

    // Reload from storage
    let records = storage.load_variables().await?;
    assert_eq!(records.len(), 2);

    let keys: Vec<&str> = records.iter().map(|r| r.key.as_str()).collect();
    assert!(keys.contains(&"A"));
    assert!(keys.contains(&"B"));
    Ok(())
}

#[tokio::test]
async fn scope_resolution_priority() -> Result {
    let tmp = tempfile::tempdir()?;
    let storage: Arc<dyn Storage> = Arc::new(FsStorage::new(tmp.path().to_path_buf()));

    let records = vec![
        VariableRecord {
            key: "KEY".into(),
            value: "global_val".into(),
            scope: VariableScope::Global,
            project_id: None,
            session_id: None,
            secret: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            used_count: 0,
            last_used_at: None,
            last_used_by: None,
        },
        VariableRecord {
            key: "KEY".into(),
            value: "project_val".into(),
            scope: VariableScope::Project,
            project_id: Some("/my/project".into()),
            session_id: None,
            secret: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            used_count: 0,
            last_used_at: None,
            last_used_by: None,
        },
        VariableRecord {
            key: "KEY".into(),
            value: "session_val".into(),
            scope: VariableScope::Session,
            project_id: None,
            session_id: Some("sess_1".into()),
            secret: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            used_count: 0,
            last_used_at: None,
            last_used_by: None,
        },
    ];

    let vars = Arc::new(Variables::new(storage, records));

    // Session wins
    let resp = vars
        .get_for_context("KEY", "/my/project", "sess_1")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(expect_found(resp)?, "session_val");

    // Without session match, project wins
    let resp = vars
        .get_for_context("KEY", "/my/project", "other_sess")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(expect_found(resp)?, "project_val");

    // Without session or project match, global wins
    let resp = vars
        .get_for_context("KEY", "/other/project", "other_sess")
        .await
        .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;
    assert_eq!(expect_found(resp)?, "global_val");
    Ok(())
}

// ---------------------------------------------------------------------------
// secret_values
// ---------------------------------------------------------------------------

#[tokio::test]
async fn secret_values_returns_secret_variable_values() -> Result {
    let tmp = tempfile::tempdir()?;
    let storage: Arc<dyn Storage> = Arc::new(FsStorage::new(tmp.path().to_path_buf()));

    let records = vec![
        VariableRecord {
            key: "SECRET_KEY".into(),
            value: "my-secret-token".into(),
            scope: VariableScope::Global,
            project_id: None,
            session_id: None,
            secret: true,
            updated_at: "2026-01-01T00:00:00Z".into(),
            used_count: 0,
            last_used_at: None,
            last_used_by: None,
        },
        VariableRecord {
            key: "PUBLIC_KEY".into(),
            value: "not-secret".into(),
            scope: VariableScope::Global,
            project_id: None,
            session_id: None,
            secret: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            used_count: 0,
            last_used_at: None,
            last_used_by: None,
        },
    ];

    let vars = Arc::new(Variables::new(storage, records));
    let secrets = vars.secret_values();
    assert_eq!(secrets, vec!["my-secret-token"]);
    Ok(())
}
