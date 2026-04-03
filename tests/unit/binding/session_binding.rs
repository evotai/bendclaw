use std::sync::Arc;

use bendclaw::binding::session_binding::bind_session;
use bendclaw::storage::local_fs::LocalFsBackend;
use bendclaw::storage::sessions::SessionRepo;

#[tokio::test]
async fn creates_new_session_when_no_id() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend as Arc<dyn SessionRepo>;

    let session = bind_session(&repo, "u01", "a01", None, false)
        .await
        .unwrap();
    assert!(!session.session_id.is_empty());
    assert_eq!(session.agent_id, "a01");
    assert_eq!(session.user_id, "u01");
}

#[tokio::test]
async fn creates_session_with_explicit_id() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend as Arc<dyn SessionRepo>;

    let session = bind_session(&repo, "u01", "a01", Some("my-session"), false)
        .await
        .unwrap();
    assert_eq!(session.session_id, "my-session");
}

#[tokio::test]
async fn resumes_existing_session() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend.clone() as Arc<dyn SessionRepo>;

    let s = bind_session(&repo, "u01", "a01", Some("s1"), false)
        .await
        .unwrap();
    assert_eq!(s.session_id, "s1");

    let resumed = bind_session(&repo, "u01", "a01", Some("s1"), true)
        .await
        .unwrap();
    assert_eq!(resumed.session_id, "s1");
}

#[tokio::test]
async fn resume_nonexistent_session_fails() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend as Arc<dyn SessionRepo>;

    let result = bind_session(&repo, "u01", "a01", Some("nope"), true).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn resume_latest_session() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend.clone() as Arc<dyn SessionRepo>;

    let _s1 = bind_session(&repo, "u01", "a01", Some("s1"), false)
        .await
        .unwrap();

    let latest = bind_session(&repo, "u01", "a01", None, true).await.unwrap();
    assert_eq!(latest.session_id, "s1");
}

#[tokio::test]
async fn resume_latest_no_sessions_fails() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend as Arc<dyn SessionRepo>;

    let result = bind_session(&repo, "u01", "a01", None, true).await;
    assert!(result.is_err());
}
