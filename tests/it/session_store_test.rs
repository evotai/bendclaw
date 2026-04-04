use bendclaw::session::SessionMeta;
use bendclaw::store::create_stores;
use bendclaw::store::StoreBackend;
use tempfile::TempDir;

#[tokio::test]
async fn save_and_load_meta() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let meta = SessionMeta::new("sess-001".into(), "/tmp".into(), "claude-sonnet".into());

    stores.session.save_meta(&meta).await.unwrap();

    let loaded = stores.session.load_meta("sess-001").await.unwrap();
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.session_id, "sess-001");
    assert_eq!(loaded.cwd, "/tmp");
    assert_eq!(loaded.model, "claude-sonnet");
    assert_eq!(loaded.turns, 0);
}

#[tokio::test]
async fn load_meta_not_found() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let loaded = stores.session.load_meta("nonexistent").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn save_and_load_transcript() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let messages = vec![
        bend_agent::Message {
            role: bend_agent::MessageRole::User,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hello".into(),
            }],
        },
        bend_agent::Message {
            role: bend_agent::MessageRole::Assistant,
            content: vec![bend_agent::ContentBlock::Text {
                text: "hi there".into(),
            }],
        },
    ];

    stores
        .session
        .save_transcript("sess-002", &messages)
        .await
        .unwrap();

    let loaded = stores.session.load_transcript("sess-002").await.unwrap();
    assert!(loaded.is_some());
    let loaded = loaded.unwrap();
    assert_eq!(loaded.len(), 2);
}

#[tokio::test]
async fn load_transcript_not_found() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let loaded = stores.session.load_transcript("nonexistent").await.unwrap();
    assert!(loaded.is_none());
}

#[tokio::test]
async fn list_recent_sessions() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    for i in 0..5 {
        let meta = SessionMeta::new(
            format!("sess-{i:03}"),
            "/tmp".into(),
            "claude-sonnet".into(),
        );
        stores.session.save_meta(&meta).await.unwrap();
    }

    let recent = stores.session.list_recent(3).await.unwrap();
    assert_eq!(recent.len(), 3);
}
