use bendclaw::conf::StorageConfig;
use bendclaw::protocol::*;
use bendclaw::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn open_storage_returns_working_backend() -> TestResult {
    let root = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(root.path().to_path_buf()))?;

    let session_meta = SessionMeta::new(
        "sess-backend".into(),
        "/tmp".into(),
        "claude-sonnet-4-20250514".into(),
    );
    storage.put_session(session_meta).await?;
    assert!(storage.get_session("sess-backend").await?.is_some());

    let run_meta = RunMeta::new(
        "run-backend".into(),
        "sess-backend".into(),
        "claude-sonnet-4-20250514".into(),
    );
    storage.put_run(run_meta).await?;

    let event = RunEvent::new(
        "run-backend".into(),
        "sess-backend".into(),
        0,
        RunEventPayload::RunStarted {},
    );
    storage.put_run_events(vec![event]).await?;

    let loaded_events = storage
        .list_run_events(ListRunEvents {
            run_id: "run-backend".into(),
        })
        .await?;
    assert_eq!(loaded_events.len(), 1);
    Ok(())
}

#[tokio::test]
async fn save_and_load_run_meta() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let meta = RunMeta::new("run-001".into(), "sess-001".into(), "claude-sonnet".into());
    storage.put_run(meta).await?;

    let path = dir
        .path()
        .join("sessions")
        .join("sess-001")
        .join("runs")
        .join("run-001.json");
    assert!(path.exists());

    let content = std::fs::read_to_string(&path)?;
    let loaded: RunMeta = serde_json::from_str(&content)?;
    assert_eq!(loaded.run_id, "run-001");
    assert_eq!(loaded.status, RunStatus::Running);
    Ok(())
}

#[tokio::test]
async fn append_and_load_events() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let first = RunEvent::new(
        "run-001".into(),
        "sess-001".into(),
        0,
        RunEventPayload::RunStarted {},
    );
    let second = RunEvent::new(
        "run-001".into(),
        "sess-001".into(),
        1,
        RunEventPayload::AssistantCompleted {
            content: vec![AssistantBlock::Text {
                text: "hello".into(),
            }],
            usage: None,
            stop_reason: "stop".into(),
            error_message: None,
        },
    );

    storage.put_run_events(vec![first, second]).await?;

    let events = storage
        .list_run_events(ListRunEvents {
            run_id: "run-001".into(),
        })
        .await?;
    assert_eq!(events.len(), 2);
    Ok(())
}

#[tokio::test]
async fn load_events_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let events = storage
        .list_run_events(ListRunEvents {
            run_id: "nonexistent".into(),
        })
        .await?;
    assert!(events.is_empty());
    Ok(())
}
