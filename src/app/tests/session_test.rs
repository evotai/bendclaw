use bendclaw::conf::StorageConfig;
use bendclaw::session::Session;
use bendclaw::storage::model::ListSessions;
use bendclaw::storage::model::ListTranscriptEntries;
use bendclaw::storage::model::SessionMeta;
use bendclaw::storage::model::TranscriptEntry;
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

    let session = Session::create(
        "sess-100".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    let meta = session.meta().await;
    let messages = session.messages().await;
    assert_eq!(meta.session_id, "sess-100");
    assert_eq!(meta.turns, 0);
    assert!(messages.is_empty());
    assert!(dir
        .path()
        .join("sessions")
        .join("sess-100")
        .join("session.json")
        .exists());
    Ok(())
}

#[tokio::test]
async fn load_session_returns_none_for_missing() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::load("nonexistent", storage.clone()).await?;
    assert!(session.is_none());
    Ok(())
}

#[tokio::test]
async fn round_trip_session_with_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::create(
        "sess-200".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .apply_messages(vec![
            bend_agent::Message {
                role: bend_agent::MessageRole::User,
                content: vec![bend_agent::ContentBlock::Text {
                    text: "hello".into(),
                }],
            },
            bend_agent::Message {
                role: bend_agent::MessageRole::Assistant,
                content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
            },
        ])
        .await;

    session.save().await?;

    let loaded = Session::load("sess-200", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing loaded session"))?;
    assert_eq!(loaded.meta().await.turns, 1);
    assert_eq!(loaded.messages().await.len(), 2);
    Ok(())
}

#[tokio::test]
async fn resume_session_appends_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::create(
        "sess-300".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .apply_messages(vec![bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "first".into(),
            }],
        }])
        .await;
    session.save().await?;

    let resumed = Session::load("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing resumed session"))?;

    let mut extended = resumed.messages().await;
    extended.push(bend_agent::Message {
        role: bend_agent::MessageRole::User,
        content: vec![bend_agent::ContentBlock::Text {
            text: "second".into(),
        }],
    });
    extended.push(bend_agent::Message {
        role: bend_agent::MessageRole::Assistant,
        content: vec![bend_agent::ContentBlock::Text {
            text: "reply".into(),
        }],
    });

    resumed.apply_messages(extended).await;
    resumed.save().await?;

    let final_state = Session::load("sess-300", storage.clone())
        .await?
        .ok_or_else(|| missing_error("missing final state"))?;
    assert_eq!(final_state.messages().await.len(), 3);
    assert_eq!(final_state.meta().await.turns, 2);
    Ok(())
}

#[tokio::test]
async fn session_title_comes_from_first_user_message() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let session = Session::create(
        "sess-title".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        storage.clone(),
    )
    .await?;

    session
        .apply_messages(vec![
            bend_agent::Message {
                role: bend_agent::MessageRole::User,
                content: vec![bend_agent::ContentBlock::Text {
                    text: "summarize the quarterly numbers for the infra team".into(),
                }],
            },
            bend_agent::Message {
                role: bend_agent::MessageRole::Assistant,
                content: vec![bend_agent::ContentBlock::Text {
                    text: "working".into(),
                }],
            },
        ])
        .await;

    session.save().await?;

    let loaded = Session::load("sess-title", storage.clone())
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
    storage.put_session(meta).await?;

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

#[tokio::test]
async fn save_and_load_transcript() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let entries = vec![
        TranscriptEntry::new("sess-002".into(), None, 1, 0, bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        }),
        TranscriptEntry::new("sess-002".into(), None, 2, 0, bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hi there".into(),
            }],
        }),
    ];

    storage.put_transcript_entries(entries).await?;

    let loaded = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: "sess-002".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert_eq!(loaded.len(), 2);
    Ok(())
}

#[tokio::test]
async fn load_transcript_not_found() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    let loaded = storage
        .list_transcript_entries(ListTranscriptEntries {
            session_id: "nonexistent".into(),
            run_id: None,
            after_seq: None,
            limit: None,
        })
        .await?;
    assert!(loaded.is_empty());
    Ok(())
}

#[tokio::test]
async fn list_recent_sessions() -> TestResult {
    let dir = TempDir::new()?;
    let storage = open_storage(&StorageConfig::fs(dir.path().to_path_buf()))?;

    for index in 0..5 {
        let meta = SessionMeta::new(
            format!("sess-{index:03}"),
            "/tmp".into(),
            "claude-sonnet".into(),
        );
        storage.put_session(meta).await?;
    }

    let recent = storage.list_sessions(ListSessions { limit: 3 }).await?;
    assert_eq!(recent.len(), 3);
    Ok(())
}
