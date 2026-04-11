//! Tests for MemoryStorage and its integration with Session.

use std::sync::Arc;

use bendclaw::agent::*;
use bendclaw::session::Session;
use bendclaw::storage::MemoryStorage;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

// ---------------------------------------------------------------------------
// MemoryStorage basics
// ---------------------------------------------------------------------------

#[tokio::test]
async fn memory_storage_save_and_get_session() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    let meta = SessionMeta::new("mem-001".into(), "/tmp".into(), "test-model".into());
    storage.save_session(meta).await?;

    let loaded = storage.get_session("mem-001").await?;
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.session_id, "mem-001");
    assert_eq!(loaded.model, "test-model");
    Ok(())
}

#[tokio::test]
async fn memory_storage_get_missing_session_returns_none() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());
    let loaded = storage.get_session("nonexistent").await?;
    assert!(loaded.is_none());
    Ok(())
}

#[tokio::test]
async fn memory_storage_append_and_list_entries() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    storage
        .append_entry(TranscriptEntry::new(
            "mem-002".into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "hello".into(),
            },
        ))
        .await?;
    storage
        .append_entry(TranscriptEntry::new(
            "mem-002".into(),
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

    let entries = storage
        .list_entries(ListTranscriptEntries {
            session_id: "mem-002".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0].item, TranscriptItem::User { text } if text == "hello"));
    assert!(matches!(&entries[1].item, TranscriptItem::Assistant { text, .. } if text == "hi"));
    Ok(())
}

#[tokio::test]
async fn memory_storage_filters_entries_by_session_id() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    storage
        .append_entry(TranscriptEntry::new(
            "sess-a".into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "from a".into(),
            },
        ))
        .await?;
    storage
        .append_entry(TranscriptEntry::new(
            "sess-b".into(),
            None,
            1,
            0,
            TranscriptItem::User {
                text: "from b".into(),
            },
        ))
        .await?;

    let entries_a = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-a".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries_a.len(), 1);
    assert!(matches!(&entries_a[0].item, TranscriptItem::User { text } if text == "from a"));

    let entries_b = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-b".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(entries_b.len(), 1);
    assert!(matches!(&entries_b[0].item, TranscriptItem::User { text } if text == "from b"));
    Ok(())
}

#[tokio::test]
async fn memory_storage_list_sessions() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    storage
        .save_session(SessionMeta::new("s1".into(), "/tmp".into(), "m".into()))
        .await?;
    storage
        .save_session(SessionMeta::new("s2".into(), "/tmp".into(), "m".into()))
        .await?;

    let sessions = storage.list_sessions(ListSessions { limit: 10 }).await?;
    assert_eq!(sessions.len(), 2);
    Ok(())
}

// ---------------------------------------------------------------------------
// MemoryStorage + Session integration
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_new_with_memory_storage() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    let session = Session::new(
        "mem-sess-001".into(),
        "/tmp".into(),
        "test-model".into(),
        storage.clone(),
    )
    .await?;

    let meta = session.meta().await;
    assert_eq!(meta.session_id, "mem-sess-001");
    assert!(session.transcript().await.is_empty());
    Ok(())
}

#[tokio::test]
async fn session_write_and_read_transcript_in_memory() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    let session = Session::new(
        "mem-sess-002".into(),
        "/tmp".into(),
        "test-model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
            },
            TranscriptItem::Assistant {
                text: "hi".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    let transcript = session.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text } if text == "hello"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "hi"));
    Ok(())
}

#[tokio::test]
async fn session_open_with_memory_storage_restores_transcript() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    // Create and populate a session
    let session = Session::new(
        "mem-sess-003".into(),
        "/tmp".into(),
        "test-model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "first".into(),
            },
            TranscriptItem::Assistant {
                text: "reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;
    session.save().await?;

    // Re-open the same session — should restore transcript from MemoryStorage
    let reopened = Session::open("mem-sess-003", storage.clone())
        .await?
        .ok_or("session not found after reopen")?;

    let transcript = reopened.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text } if text == "first"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "reply"));
    Ok(())
}

#[tokio::test]
async fn session_multi_turn_with_memory_storage() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    // Turn 1: create session, write items
    let session = Session::new(
        "mem-sess-multi".into(),
        "/tmp".into(),
        "test-model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "turn 1 question".into(),
            },
            TranscriptItem::Assistant {
                text: "turn 1 answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;
    session.save().await?;

    // Turn 2: reopen session, verify context, write more
    let session2 = Session::open("mem-sess-multi", storage.clone())
        .await?
        .ok_or("session not found for turn 2")?;

    // Should have turn 1 context
    assert_eq!(session2.transcript().await.len(), 2);

    session2
        .write_items(vec![
            TranscriptItem::User {
                text: "turn 2 question".into(),
            },
            TranscriptItem::Assistant {
                text: "turn 2 answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;
    session2.save().await?;

    // Turn 3: reopen again, should have all 4 items
    let session3 = Session::open("mem-sess-multi", storage.clone())
        .await?
        .ok_or("session not found for turn 3")?;

    let transcript = session3.transcript().await;
    assert_eq!(transcript.len(), 4);
    assert!(matches!(&transcript[0], TranscriptItem::User { text } if text == "turn 1 question"));
    assert!(
        matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "turn 1 answer")
    );
    assert!(matches!(&transcript[2], TranscriptItem::User { text } if text == "turn 2 question"));
    assert!(
        matches!(&transcript[3], TranscriptItem::Assistant { text, .. } if text == "turn 2 answer")
    );
    Ok(())
}

#[tokio::test]
async fn memory_storage_is_isolated_between_instances() -> TestResult {
    let storage1: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());
    let storage2: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    storage1
        .save_session(SessionMeta::new(
            "isolated".into(),
            "/tmp".into(),
            "m".into(),
        ))
        .await?;

    // storage2 should not see storage1's data
    assert!(storage2.get_session("isolated").await?.is_none());
    Ok(())
}

#[tokio::test]
async fn memory_storage_dropped_leaves_no_trace() -> TestResult {
    let storage: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());

    let session = Session::new(
        "ephemeral".into(),
        "/tmp".into(),
        "m".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "side chat".into(),
        }])
        .await?;

    // Verify data exists
    assert!(storage.get_session("ephemeral").await?.is_some());

    // Drop everything
    drop(session);
    drop(storage);

    // A new MemoryStorage has no data — confirms nothing leaked to disk
    let fresh: Arc<dyn bendclaw::storage::Storage> = Arc::new(MemoryStorage::new());
    assert!(fresh.get_session("ephemeral").await?.is_none());
    Ok(())
}
