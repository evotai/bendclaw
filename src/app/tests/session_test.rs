use evot::agent::session::Session;
use evot::agent::*;
use evot::conf::StorageConfig;
use evot::storage::open_storage;
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
                content: vec![],
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
            content: vec![],
        }])
        .await?;

    let resumed = Session::open("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing resumed session"))?;

    resumed
        .write_items(vec![
            TranscriptItem::User {
                text: "second".into(),
                content: vec![],
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
                content: vec![],
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
                content: vec![],
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
    assert!(matches!(&loaded[0].item, TranscriptItem::User { text, .. } if text == "hello"));
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
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "old reply 1".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
            TranscriptItem::User {
                text: "old message 2".into(),
                content: vec![],
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
                    content: vec![],
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
                content: vec![],
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
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "summary of prior context")
    );
    assert!(
        matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "acknowledged")
    );
    assert!(
        matches!(&transcript[2], TranscriptItem::User { text, .. } if text == "new message after compact")
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
                content: vec![],
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
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "hello"));
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
            content: vec![],
        }])
        .await?;

    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![TranscriptItem::User {
                text: "compacted".into(),
                content: vec![],
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
                content: vec![],
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
                content: vec![],
            }],
        }])
        .await?;

    // More messages
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg2".into(),
            content: vec![],
        }])
        .await?;

    // Second compaction
    session
        .write_items(vec![TranscriptItem::Compact {
            messages: vec![TranscriptItem::User {
                text: "compact-v2".into(),
                content: vec![],
            }],
        }])
        .await?;

    // One more message after second compaction
    session
        .write_items(vec![TranscriptItem::User {
            text: "msg3".into(),
            content: vec![],
        }])
        .await?;

    // Load should use the second (last) compact
    let loaded = Session::open("sess-multi-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;

    // compact-v2 messages (1) + msg3 (1) = 2
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "compact-v2"));
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "msg3"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Stats filtering on resume
// ---------------------------------------------------------------------------

#[tokio::test]
async fn stats_items_persisted_but_filtered_on_resume() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-stats".into(),
        "/tmp".into(),
        "m".into(),
        storage.clone(),
    )
    .await?;

    // Write a mix of conversation items and stats
    let stats_item =
        evot::types::TranscriptStats::LlmCallCompleted(evot::types::LlmCallCompletedStats {
            turn: 1,
            attempt: 0,
            usage: evot::types::UsageSummary {
                input: 100,
                output: 50,
                cache_read: 0,
                cache_write: 0,
            },
            metrics: None,
            error: None,
        })
        .to_item();

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            stats_item,
            TranscriptItem::Assistant {
                text: "hi".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "end_turn".into(),
            },
        ])
        .await?;
    session.save().await?;

    // Raw storage should have 3 entries
    let raw = storage
        .list_entries(evot::types::ListTranscriptEntries {
            session_id: "sess-stats".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(raw.len(), 3);
    assert!(
        matches!(&raw[1].item, TranscriptItem::Stats { kind, .. } if kind == "llm_call_completed")
    );

    // Resumed session transcript should only have 2 items (no stats)
    let loaded = Session::open("sess-stats", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "hello"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "hi"));
    Ok(())
}

#[tokio::test]
async fn stats_after_compact_filtered_on_resume() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;
    let session = Session::new(
        "sess-stats-compact".into(),
        "/tmp".into(),
        "m".into(),
        storage.clone(),
    )
    .await?;

    // Write initial messages
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old msg".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "old reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "end_turn".into(),
            },
        ])
        .await?;

    // Write compact + stats + new message
    let compact_stats = evot::types::TranscriptStats::ContextCompactionCompleted(
        evot::types::ContextCompactionCompletedStats {
            result: evot::types::CompactionResult::LevelCompacted {
                level: 1,
                before_message_count: 10,
                after_message_count: 4,
                before_estimated_tokens: 30000,
                after_estimated_tokens: 12000,
                tool_outputs_truncated: 2,
                turns_summarized: 3,
                messages_dropped: 1,
                oversize_capped: 0,
                age_cleared: 0,
                actions: vec![],
            },
        },
    )
    .to_item();

    session
        .write_items(vec![
            TranscriptItem::Compact {
                messages: vec![TranscriptItem::User {
                    text: "summary".into(),
                    content: vec![],
                }],
            },
            compact_stats,
            TranscriptItem::User {
                text: "new msg".into(),
                content: vec![],
            },
        ])
        .await?;
    session.save().await?;

    // Resume: should see compact base + new msg, no stats
    let loaded = Session::open("sess-stats-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "summary"));
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "new msg"));
    Ok(())
}

// ---------------------------------------------------------------------------
// Planning mode — user input must not be polluted by planning prompt
// ---------------------------------------------------------------------------

/// The old bug: planning prompt was prepended to user input and stored as a
/// single User transcript item. `first_user_title` then picked up the planning
/// prompt as the session title. This test reproduces the old bug scenario and
/// proves that a polluted User message yields a wrong title.
#[tokio::test]
async fn title_is_wrong_when_planning_prompt_pollutes_user_message() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-old-bug".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    // Reproduce the OLD behavior: planning prompt + user input in one message.
    let polluted = format!(
        "You are in planning mode\n\nUser task:\n{}",
        "refactor the auth module to use JWT"
    );
    session
        .write_items(vec![TranscriptItem::User {
            text: polluted,
            content: vec![],
        }])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-old-bug", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing title"))?;

    // Title starts with planning prompt — this is the bug we fixed.
    assert!(title.starts_with("You are in planning mode"));
    assert!(!title.contains("refactor the auth module"));
    Ok(())
}

/// After the fix, planning prompt lives in system_prompt, not in the user
/// message. When run_loop stores only the raw user input, `first_user_title`
/// derives the correct title.
#[tokio::test]
async fn title_is_correct_when_user_message_is_clean() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-plan".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    // The NEW behavior: only raw user input in the transcript.
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "refactor the auth module to use JWT".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "planning".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;
    session.save().await?;

    let loaded = Session::open("sess-plan", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing planning session"))?;
    let title = loaded
        .meta()
        .await
        .title
        .ok_or_else(|| missing_error("missing session title"))?;

    assert_eq!(title, "refactor the auth module to use JWT");
    Ok(())
}

// ---------------------------------------------------------------------------
// Marker tests — /clear, /goto, new Compact marker
// ---------------------------------------------------------------------------

#[tokio::test]
async fn clear_marker_resets_context() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-clear".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg1".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "reply1".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    session.write_clear_marker().await?;

    // In-memory transcript should be empty after clear
    assert!(session.transcript().await.is_empty());

    // New messages after clear
    session
        .write_items(vec![TranscriptItem::User {
            text: "fresh start".into(),
            content: vec![],
        }])
        .await?;

    // Reload from storage — should only see post-clear messages
    let loaded = Session::open("sess-clear", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 1);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "fresh start"));
    Ok(())
}

#[tokio::test]
async fn goto_marker_restores_snapshot() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-goto".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    // seq 1-4: two turns
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg1".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "reply1".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
            TranscriptItem::User {
                text: "msg2".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "reply2".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // goto seq 2 — should restore context to [msg1, reply1]
    session.write_goto_marker(2).await?;

    let transcript = session.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "msg1"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "reply1"));

    // Continue from goto point
    session
        .write_items(vec![TranscriptItem::User {
            text: "new direction".into(),
            content: vec![],
        }])
        .await?;

    // Reload — should see snapshot(msg1, reply1) + new message
    let loaded = Session::open("sess-goto", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 3);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "msg1"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "reply1"));
    assert!(matches!(&transcript[2], TranscriptItem::User { text, .. } if text == "new direction"));
    Ok(())
}

#[tokio::test]
async fn goto_after_clear_restores_old_context() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-goto-after-clear".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "original".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "response".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    session.write_clear_marker().await?;
    assert!(session.transcript().await.is_empty());

    // goto back to seq 2 — should recover the original context
    session.write_goto_marker(2).await?;
    let transcript = session.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "original"));
    assert!(matches!(&transcript[1], TranscriptItem::Assistant { text, .. } if text == "response"));
    Ok(())
}

#[tokio::test]
async fn new_compact_marker_works_like_old_compact() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-new-compact".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "old reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Write new-style compact marker
    session
        .write_compact_marker(vec![TranscriptItem::User {
            text: "compacted summary".into(),
            content: vec![],
        }])
        .await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "after compact".into(),
            content: vec![],
        }])
        .await?;

    // Reload — should see compact snapshot + new message
    let loaded = Session::open("sess-new-compact", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;
    let transcript = loaded.transcript().await;
    assert_eq!(transcript.len(), 2);
    assert!(
        matches!(&transcript[0], TranscriptItem::User { text, .. } if text == "compacted summary")
    );
    assert!(matches!(&transcript[1], TranscriptItem::User { text, .. } if text == "after compact"));
    Ok(())
}

#[tokio::test]
async fn is_valid_context_seq_checks_correctly() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-valid-seq".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "msg".into(),
                content: vec![],
            },
            TranscriptItem::Stats {
                kind: "test".into(),
                data: serde_json::json!({}),
            },
        ])
        .await?;

    // seq 1 is a User message — valid
    assert!(session.is_valid_context_seq(1).await?);
    // seq 2 is a Stats item — not valid
    assert!(!session.is_valid_context_seq(2).await?);
    // seq 99 doesn't exist — not valid
    assert!(!session.is_valid_context_seq(99).await?);
    Ok(())
}

#[tokio::test]
async fn marker_item_is_not_context() {
    let item = TranscriptItem::Marker {
        kind: evot::types::MarkerKind::Clear,
        target_seq: None,
        messages: vec![],
    };
    assert!(!item.is_context_item());

    let item = TranscriptItem::Marker {
        kind: evot::types::MarkerKind::Goto,
        target_seq: Some(5),
        messages: vec![],
    };
    assert!(!item.is_context_item());
}

#[tokio::test]
async fn history_on_resumed_session() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    // Create session with some messages
    let session = Session::new(
        "sess-history".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "hi there".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
            TranscriptItem::User {
                text: "how are you".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "doing well".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Reopen (resume) the session
    let loaded = Session::open("sess-history", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;

    // Call recent_context_entries
    let entries = loaded.recent_context_entries(20).await?;
    assert_eq!(entries.len(), 4);
    assert_eq!(entries[0].0, 1); // seq
    assert!(matches!(&entries[0].1, TranscriptItem::User { text, .. } if text == "hello"));
    assert_eq!(entries[1].0, 2);
    assert!(matches!(&entries[1].1, TranscriptItem::Assistant { text, .. } if text == "hi there"));
    assert_eq!(entries[2].0, 3);
    assert_eq!(entries[3].0, 4);

    // Limit works
    let last2 = loaded.recent_context_entries(2).await?;
    assert_eq!(last2.len(), 2);
    assert_eq!(last2[0].0, 3);
    assert_eq!(last2[1].0, 4);
    Ok(())
}

#[tokio::test]
async fn history_after_clear_shows_only_post_clear() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-history-clear".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "old msg".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "old reply".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    session.write_clear_marker().await?;

    session
        .write_items(vec![TranscriptItem::User {
            text: "new msg".into(),
            content: vec![],
        }])
        .await?;

    // Reopen
    let loaded = Session::open("sess-history-clear", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;

    // History should only show post-clear messages
    let entries = loaded.recent_context_entries(20).await?;
    assert_eq!(entries.len(), 1);
    assert!(matches!(&entries[0].1, TranscriptItem::User { text, .. } if text == "new msg"));
    Ok(())
}

#[tokio::test]
async fn history_after_goto_shows_snapshot_and_new_entries() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-goto-history".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    // Write 4 messages: 2 turns
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "first question".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "first answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
            TranscriptItem::User {
                text: "second question".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "second answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Goto seq 2 (first assistant answer)
    session.write_goto_marker(2).await?;

    // Write new messages after goto
    session
        .write_items(vec![
            TranscriptItem::User {
                text: "new question".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "new answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // Reopen session
    let loaded = Session::open("sess-goto-history", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing session"))?;

    let entries = loaded.recent_context_entries(20).await?;

    // Should see: snapshot baseline (seq=0) + new entries (real seq)
    // Snapshot has 2 items: user "first question", assistant "first answer"
    // New has 2 items: user "new question", assistant "new answer"
    assert_eq!(entries.len(), 4);

    // Snapshot items have seq=0
    assert_eq!(entries[0].0, 0);
    assert!(matches!(&entries[0].1, TranscriptItem::User { text, .. } if text == "first question"));
    assert_eq!(entries[1].0, 0);
    assert!(
        matches!(&entries[1].1, TranscriptItem::Assistant { text, .. } if text == "first answer")
    );

    // New items have real seq
    assert!(entries[2].0 > 0);
    assert!(matches!(&entries[2].1, TranscriptItem::User { text, .. } if text == "new question"));
    assert!(entries[3].0 > 0);
    assert!(
        matches!(&entries[3].1, TranscriptItem::Assistant { text, .. } if text == "new answer")
    );

    Ok(())
}

#[tokio::test]
async fn history_excludes_empty_messages() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-empty-msg".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "".into(), // empty assistant (tool call start)
                thinking: None,
                tool_calls: vec![],
                stop_reason: "tool_use".into(),
            },
            TranscriptItem::Assistant {
                text: "real answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    let entries = session.recent_context_entries(20).await?;

    // Empty assistant should be filtered out
    assert_eq!(entries.len(), 2);
    assert!(matches!(&entries[0].1, TranscriptItem::User { text, .. } if text == "hello"));
    assert!(
        matches!(&entries[1].1, TranscriptItem::Assistant { text, .. } if text == "real answer")
    );

    Ok(())
}

#[tokio::test]
async fn is_valid_context_seq_rejects_empty_assistant() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-valid-seq".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "tool_use".into(),
            },
            TranscriptItem::Assistant {
                text: "real answer".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    // seq 1 = user "hello" → valid (user message)
    assert!(session.is_valid_context_seq(1).await?);
    // seq 2 = empty assistant → invalid (not a user message)
    assert!(!session.is_valid_context_seq(2).await?);
    // seq 3 = assistant "real answer" → invalid (not a user message)
    assert!(!session.is_valid_context_seq(3).await?);

    Ok(())
}

#[tokio::test]
async fn get_item_at_returns_correct_item() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::new(
        "sess-get-item".into(),
        "/tmp".into(),
        "model".into(),
        storage.clone(),
    )
    .await?;

    session
        .write_items(vec![
            TranscriptItem::User {
                text: "hello".into(),
                content: vec![],
            },
            TranscriptItem::Assistant {
                text: "world".into(),
                thinking: None,
                tool_calls: vec![],
                stop_reason: "stop".into(),
            },
        ])
        .await?;

    let item = session.get_item_at(1).await?;
    assert!(matches!(&item, Some(TranscriptItem::User { text, .. }) if text == "hello"));

    let item = session.get_item_at(2).await?;
    assert!(matches!(&item, Some(TranscriptItem::Assistant { text, .. }) if text == "world"));

    let item = session.get_item_at(999).await?;
    assert!(item.is_none());

    Ok(())
}
