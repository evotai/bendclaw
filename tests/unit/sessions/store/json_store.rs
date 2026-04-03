//! Tests for JsonSessionStore — local JSON file persistence.

use bendclaw::sessions::store::json::JsonSessionStore;
use bendclaw::sessions::store::SessionStore;
use bendclaw::storage::dal::run::record::RunRecord;
use bendclaw::storage::dal::run::record::RunStatus;
use bendclaw::storage::dal::session::repo::SessionWrite;

fn temp_store() -> (tempfile::TempDir, JsonSessionStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = JsonSessionStore::new(dir.path().to_path_buf());
    (dir, store)
}

fn test_session_write(session_id: &str) -> SessionWrite {
    SessionWrite {
        session_id: session_id.to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "user-1".to_string(),
        title: "test session".to_string(),
        base_key: String::new(),
        replaced_by_session_id: String::new(),
        reset_reason: String::new(),
        session_state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
    }
}

fn test_run_record(run_id: &str, session_id: &str) -> RunRecord {
    RunRecord {
        id: run_id.to_string(),
        session_id: session_id.to_string(),
        agent_id: "agent-1".to_string(),
        user_id: "user-1".to_string(),
        kind: "user_turn".to_string(),
        parent_run_id: String::new(),
        node_id: String::new(),
        status: RunStatus::Running.as_str().to_string(),
        input: "hello".to_string(),
        output: String::new(),
        error: String::new(),
        metrics: String::new(),
        stop_reason: String::new(),
        checkpoint_through_run_id: String::new(),
        iterations: 0,
        created_at: String::new(),
        updated_at: String::new(),
    }
}

#[tokio::test]
async fn session_upsert_and_load() {
    let (_dir, store) = temp_store();
    let write = test_session_write("s1");
    store.session_upsert(write).await.unwrap();

    let loaded = store.session_load("s1").await.unwrap();
    assert!(loaded.is_some());
    let rec = loaded.unwrap();
    assert_eq!(rec.id, "s1");
    assert_eq!(rec.agent_id, "agent-1");
    assert_eq!(rec.title, "test session");
}

#[tokio::test]
async fn session_load_missing_returns_none() {
    let (_dir, store) = temp_store();
    let loaded = store.session_load("nonexistent").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn run_insert_and_list() {
    let (_dir, store) = temp_store();
    let r1 = test_run_record("r1", "s1");
    let r2 = test_run_record("r2", "s1");
    store.run_insert(&r1).await.unwrap();
    store.run_insert(&r2).await.unwrap();

    let runs = store.run_list_by_session("s1", 10).await.unwrap();
    assert_eq!(runs.len(), 2);
}

#[tokio::test]
async fn run_update_final() {
    let (_dir, store) = temp_store();
    let r = test_run_record("r1", "s1");
    store.run_insert(&r).await.unwrap();

    store
        .run_update_final(
            "r1",
            RunStatus::Completed,
            "output",
            "",
            "{}",
            "end_turn",
            3,
        )
        .await
        .unwrap();

    let runs = store.run_list_by_session("s1", 10).await.unwrap();
    let updated = &runs[0];
    assert_eq!(updated.output, "output");
    assert_eq!(updated.status, "COMPLETED");
    assert_eq!(updated.iterations, 3);
}

#[tokio::test]
async fn run_update_status() {
    let (_dir, store) = temp_store();
    let r = test_run_record("r1", "s1");
    store.run_insert(&r).await.unwrap();

    store
        .run_update_status("r1", RunStatus::Cancelled)
        .await
        .unwrap();

    let runs = store.run_list_by_session("s1", 10).await.unwrap();
    assert_eq!(runs[0].status, "CANCELLED");
}

#[tokio::test]
async fn usage_flush_is_noop() {
    let (_dir, store) = temp_store();
    // Local store has no buffering — flush is a no-op
    store.usage_flush().await.unwrap();
}
