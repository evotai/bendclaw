use std::sync::Arc;

use bendclaw::kernel::run::persist::run_cleanup::cleanup;
use bendclaw::kernel::run::persist::run_cleanup::CleanupPolicy;
use bendclaw::storage::local_fs::LocalFsBackend;
use bendclaw::storage::runs::RunRepo;
use bendclaw::types::entities::Run;
use bendclaw::types::entities::RunStatus;

fn ts() -> String {
    "2026-01-01T00:00:00Z".to_string()
}

fn make_run(run_id: &str, session_id: &str, status: &str) -> Run {
    Run {
        run_id: run_id.into(),
        session_id: session_id.into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_run_id: String::new(),
        root_trace_id: String::new(),
        kind: "user_turn".into(),
        status: status.into(),
        input: serde_json::Value::Null,
        output: serde_json::Value::Null,
        error: serde_json::Value::Null,
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        created_at: ts(),
        updated_at: ts(),
    }
}

#[tokio::test]
async fn skip_policy_does_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend as Arc<dyn RunRepo>;

    let result = cleanup(&repo, "u01", "a01", CleanupPolicy::Skip)
        .await
        .unwrap();
    assert_eq!(result.cleaned, 0);
}

#[tokio::test]
async fn full_cleanup_clears_handoffs() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend.clone() as Arc<dyn RunRepo>;

    let run = make_run("r01", "s01", RunStatus::Running.as_str());
    repo.save_run(&run).await.unwrap();
    repo.save_handoff("u01", "a01", "s01", "r01", &serde_json::json!({"turn": 3}))
        .await
        .unwrap();

    let result = cleanup(&repo, "u01", "a01", CleanupPolicy::Full)
        .await
        .unwrap();
    assert_eq!(result.cleaned, 1);

    assert!(repo
        .load_handoff("u01", "a01", "s01", "r01")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn targeted_cleanup_only_affects_session() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let repo = backend.clone() as Arc<dyn RunRepo>;

    let run1 = make_run("r01", "s01", RunStatus::Running.as_str());
    let run2 = make_run("r02", "s02", RunStatus::Running.as_str());
    repo.save_run(&run1).await.unwrap();
    repo.save_run(&run2).await.unwrap();
    repo.save_handoff("u01", "a01", "s01", "r01", &serde_json::json!({}))
        .await
        .unwrap();
    repo.save_handoff("u01", "a01", "s02", "r02", &serde_json::json!({}))
        .await
        .unwrap();

    let result = cleanup(
        &repo,
        "u01",
        "a01",
        CleanupPolicy::TargetedSession("s01".into()),
    )
    .await
    .unwrap();
    assert_eq!(result.cleaned, 1);

    // s01 handoff cleared
    assert!(repo
        .load_handoff("u01", "a01", "s01", "r01")
        .await
        .unwrap()
        .is_none());
    // s02 handoff still there
    assert!(repo
        .load_handoff("u01", "a01", "s02", "r02")
        .await
        .unwrap()
        .is_some());
}
