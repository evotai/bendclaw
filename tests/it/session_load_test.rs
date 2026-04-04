use bendclaw::session::load_session;
use bendclaw::session::new_session;
use bendclaw::session::save_transcript;
use bendclaw::session::update_transcript;
use bendclaw::store::create_stores;
use bendclaw::store::StoreBackend;
use tempfile::TempDir;

#[tokio::test]
async fn new_session_creates_meta_and_empty_transcript() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let state = new_session(
        "sess-100".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        stores.session.as_ref(),
    )
    .await
    .unwrap();

    assert_eq!(state.meta.session_id, "sess-100");
    assert_eq!(state.meta.turns, 0);
    assert!(state.messages.is_empty());

    let meta_path = dir.path().join("sess-100.json");
    assert!(meta_path.exists());
}

#[tokio::test]
async fn load_session_returns_none_for_missing() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let result = load_session("nonexistent", stores.session.as_ref())
        .await
        .unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn round_trip_session_with_transcript() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let mut state = new_session(
        "sess-200".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        stores.session.as_ref(),
    )
    .await
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
            content: vec![bend_agent::ContentBlock::Text { text: "hi".into() }],
        },
    ];

    update_transcript(&mut state, messages);
    assert_eq!(state.meta.turns, 1);
    assert_eq!(state.messages.len(), 2);

    save_transcript(&state, stores.session.as_ref())
        .await
        .unwrap();

    let loaded = load_session("sess-200", stores.session.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(loaded.meta.turns, 1);
    assert_eq!(loaded.messages.len(), 2);
}

#[tokio::test]
async fn resume_session_appends_transcript() {
    let dir = TempDir::new().unwrap();
    let stores = create_stores(StoreBackend::Fs {
        session_dir: dir.path().to_path_buf(),
        run_dir: dir.path().join("runs"),
    })
    .unwrap();

    let mut state = new_session(
        "sess-300".into(),
        "/tmp".into(),
        "claude-sonnet".into(),
        stores.session.as_ref(),
    )
    .await
    .unwrap();

    let first_messages = vec![bend_agent::Message {
        role: bend_agent::MessageRole::User,
        content: vec![bend_agent::ContentBlock::Text {
            text: "first".into(),
        }],
    }];
    update_transcript(&mut state, first_messages);
    save_transcript(&state, stores.session.as_ref())
        .await
        .unwrap();

    let mut resumed = load_session("sess-300", stores.session.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(resumed.messages.len(), 1);

    let mut extended = resumed.messages.clone();
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

    update_transcript(&mut resumed, extended);
    save_transcript(&resumed, stores.session.as_ref())
        .await
        .unwrap();

    let final_state = load_session("sess-300", stores.session.as_ref())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_state.messages.len(), 3);
    assert_eq!(final_state.meta.turns, 2);
}
