use bendclaw::run::RunEvent;
use bendclaw::run::RunEventKind;
use bendclaw::run::RunMeta;
use bendclaw::run::RunStatus;
use bendclaw::store::create_stores;
use bendclaw::store::StoreBackend;
use tempfile::TempDir;

#[tokio::test]
async fn save_and_load_run_meta() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().join("sessions"),
        run_dir: dir.path().to_path_buf(),
    })
    .unwrap();

    let meta = RunMeta::new("run-001".into(), "sess-001".into(), "claude-sonnet".into());

    stores.run.save_run(&meta).await.unwrap();

    let path = dir.path().join("run-001.json");
    assert!(path.exists());

    let content = std::fs::read_to_string(&path).unwrap();
    let loaded: RunMeta = serde_json::from_str(&content).unwrap();
    assert_eq!(loaded.run_id, "run-001");
    assert_eq!(loaded.status, RunStatus::Running);
}

#[tokio::test]
async fn append_and_load_events() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().join("sessions"),
        run_dir: dir.path().to_path_buf(),
    })
    .unwrap();

    let e1 = RunEvent::new(
        "run-001".into(),
        "sess-001".into(),
        0,
        RunEventKind::RunStarted,
        serde_json::json!({}),
    );
    let e2 = RunEvent::new(
        "run-001".into(),
        "sess-001".into(),
        1,
        RunEventKind::AssistantMessage,
        serde_json::json!({"message": "hello"}),
    );

    stores.run.append_event("run-001", &e1).await.unwrap();
    stores.run.append_event("run-001", &e2).await.unwrap();

    let events = stores.run.load_events("run-001").await.unwrap();
    assert_eq!(events.len(), 2);
}

#[tokio::test]
async fn load_events_not_found() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().join("sessions"),
        run_dir: dir.path().to_path_buf(),
    })
    .unwrap();

    let events = stores.run.load_events("nonexistent").await.unwrap();
    assert!(events.is_empty());
}
