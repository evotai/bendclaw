use bendclaw::run::RunEvent;
use bendclaw::run::RunEventKind;
use bendclaw::run::RunMeta;
use bendclaw::session::SessionMeta;
use bendclaw::store::create_stores;
use bendclaw::store::StoreBackend;
use tempfile::TempDir;

#[tokio::test]
async fn create_stores_returns_working_backends() -> Result<(), Box<dyn std::error::Error>> {
    let root = TempDir::new()?;
    let session_dir = root.path().join("sessions");
    let run_dir = root.path().join("runs");

    let stores = create_stores(StoreBackend::Fs {
        session_dir,
        run_dir,
    })?;

    let session_meta = SessionMeta::new(
        "sess-backend".into(),
        "/tmp".into(),
        "claude-sonnet-4-20250514".into(),
    );
    stores.session.save_meta(&session_meta).await?;

    let loaded_session = stores.session.load_meta("sess-backend").await?;
    assert!(loaded_session.is_some());

    let run_meta = RunMeta::new(
        "run-backend".into(),
        "sess-backend".into(),
        "claude-sonnet-4-20250514".into(),
    );
    stores.run.save_run(&run_meta).await?;

    let event = RunEvent::new(
        "run-backend".into(),
        "sess-backend".into(),
        0,
        RunEventKind::RunStarted,
        serde_json::json!({}),
    );
    stores.run.append_event("run-backend", &event).await?;

    let loaded_events = stores.run.load_events("run-backend").await?;
    assert_eq!(loaded_events.len(), 1);

    Ok(())
}
