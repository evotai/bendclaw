use bendclaw::agent::*;
use bendclaw::conf::StorageConfig;
use bendclaw::session::Session;
use bendclaw::storage::open_storage;
use tempfile::TempDir;

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

fn missing_error(message: &str) -> std::io::Error {
    std::io::Error::other(message.to_string())
}

#[tokio::test]
async fn new_session_creates_meta_and_empty_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-100".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    let meta = session.meta().await;
    let transcript = session.transcript().await;
    assert_eq!(meta.session_id, "sess-100");
    assert_eq!(meta.turns, 0);
    assert!(transcript.is_empty());
    assert!(dir
        .path()
        .join("sessions")
        .join("sess-100")
        .join("session.json")
        .exists());
    Ok(())
}

#[tokio::test]
async fn open_session_returns_none_for_missing() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::open("nonexistent", storage.clone()).await?;
    assert!(session.is_none());
    Ok(())
}

#[tokio::test]
async fn round_trip_session_with_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-200".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
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

    let loaded = Session::open("sess-200", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing loaded session"))?;
    assert_eq!(loaded.meta().await.turns, 0);
    assert_eq!(loaded.transcript().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn resume_session_appends_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-300".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "first".into(),
        }])
        .await?;

    let resumed = Session::open("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing resumed session"))?;

    resumed
        .write_items(vec![
            TranscriptItem::User {
                text: "second".into(),
            },
            TranscriptItem::Assistant {
                text: "reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    let final_state = Session::open("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing final state"))?;
    assert_eq!(final_state.transcript().await.len(), 3);
    assert_eq!(final_state.meta().await.turns, 0);
    Ok(())
}

#[tokio::test]
async fn session_title_comes_from_first_user_message() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-title".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "summarize the quarterly numbers for the infra team".into(),
            },
            TranscriptItem::Assistant {
                text: "working".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-title", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing titled session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing session title"))?;

    assert_eq!(title, "summarize the quarterly numbers for the infra team");
    Ok(())
}

#[tokio::test]
async fn save_and_load_meta() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let meta = SessionMeta::new("sess-001".into(), "/tmp".into(), "claude-sonnet".into());
    storage.save_session(meta).await?;

    let loaded = storage
        .get_session("sess-001")
        .await?
        .ok_or_else(|| missing_error("missing session meta"))?;
    assert_eq!(loaded.session_id, "sess-001");
    assert_eq!(loaded.cwd, "/tmp");
    assert_eq!(loaded.model, "claude-sonnet");
    assert_eq!(loaded.turns, 0);
    Ok(())
}

#[tokio::test]
async fn load_meta_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let loaded = storage.get_session("nonexistent").await?;
    assert!(loaded.is_none());
    Ok(())
}

// --- PLACEHOLDER_REST ---

#[tokio::test]
async fn save_and_load_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    storage
        .append_entry(TranscriptEntry::new(
            "sess-002".into(),
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
            "sess-002".into(),
            None,
            2,
            0,
            TranscriptItem::Assistant {
                text: "hi there".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ))
        .await?;

    let loaded = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-002".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    assert!(matches!(&loaded[0].item, TranscriptItem::User { text } if text == "hello"));
    assert!(
        matches!(&loaded[1].item, TranscriptItem::Assistant { text, .. } if text == "hi there")
    );
    Ok(())
}

#[tokio::test]
async fn open_resumes_from_last_compact_entry() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old message 1".into(),
            },
            TranscriptItem::Assistant {
                text: "old reply 1".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
            TranscriptItem::User {
                text: "old message 2".into(),
            },
            TranscriptItem::Assistant {
                text: "old reply 2".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Append a Compact entry (simulating compaction)
    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![
                TranscriptItem::User {
                    text: "summary of prior context".into(),
                },
                TranscriptItem::Assistant {
                    text: "acknowledged".into(),
                    thinking: None,
                    tool_calls: vec![],
                    stop_reason: "stop".into(),
                },
            ],
        }])
        .await?;

    // Append more messages after compaction
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "new message after compact".into(),
            },
            TranscriptItem::Assistant {
                text: "new reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Load — should resume from the Compact snapshot
    let loaded = Session::open("sess-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing compacted session"))?;
    let transcript = loaded.transcript().await;

    // Should have: 2 from compact + 2 new = 4 (not the original 4 + compact + 2)
    assert_eq!(transcript.len(), 4);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text } if text == "summary of prior context")
    );
    assert!(
        matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "acknowledged")
    );
    assert!(
        matches!(&transcript[2], TranscriptItem::User { text } if text == "new message after compact")
    );
    assert!(
        matches!(&transcript[3], TranscriptItem::Assistant { text, .. } if text == "new reply")
    );
    Ok(())
}

#[tokio::test]
async fn open_without_compact_returns_all_entries() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-no-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
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

    let loaded = Session::open("sess-no-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text } if text == "hello"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "hi"));
    Ok(())
}

#[tokio::test]
async fn write_items_is_append_only() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-append".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "first".into(),
        }])
        .await?;

    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![TranscriptItem::User {
                text: "compacted".into(),
            }],
        }])
        .await?;

    // Raw storage should have 2 entries (User + Compact), not a rewrite
    let raw = storage
        .list_entries(ListTranscriptEntries {
            session_id: "sess-append".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(raw.len(), 2);
    assert!(matches!(&raw[0].item, TranscriptItem::User { .. }));
    assert!(matches!(&raw[1].item, TranscriptItem::Compact { .. }));
    Ok(())
}

#[tokio::test]
async fn multiple_compactions_uses_last() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-multi-compact".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg1".into(),
            },
            TranscriptItem::Assistant {
                text: "reply1".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // First compaction
    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![TranscriptItem::User {
                text: "compact-v1".into(),
            }],
        }])
        .await?;

    // More messages
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg2".into(),
        }])
        .await?;

    // Second compaction
    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![TranscriptItem::User {
                text: "compact-v2".into(),
            }],
        }])
        .await?;

    // One more message after second compaction
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg3".into(),
        }])
        .await?;

    // Load should use the second (last) compact
    let loaded = Session::open("sess-multi-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;

    // compact-v2 messages (1) + msg3 (1) = 2
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text } if text == "compact-v2"));
    assert!(matches!(&transcript[1], TranscriptItem::User { text } if text == "msg3"));
    Ok(())
}
