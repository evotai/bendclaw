use evot::agent::*;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
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
    storage.save_session(session_meta).await?;
    assert!(storage.get_session("sess-backend").await?.is_some());

    storage
        .append_entry(TranscriptEntry::new(
            "sess-backend".into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
        ))
        .await?;
    storage
        .append_entry(TranscriptEntry::new(
            "sess-backend".into(),
            None,
            2,
            0,
            TranscriptItem::Assistant {
                text: "hi".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ))
        .await?;

    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-backend".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    Ok(())
}
