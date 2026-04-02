use std::sync::Arc;

use bendclaw::app::agent::run_executor::execute_run;
use bendclaw::app::agent::run_planner::RunPlan;
use bendclaw::storage::backend::local_fs::LocalFsBackend;
use bendclaw::storage::backend::run_event_repo::RunEventRepo;
use bendclaw::storage::backend::run_repo::RunRepo;

fn test_plan() -> RunPlan {
    RunPlan {
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        prompt: "hello world".into(),
        system_overlay: None,
        model: None,
        max_turns: Some(10),
        max_duration_secs: Some(60),
        tool_filter: None,
    }
}

#[tokio::test]
async fn execute_creates_run_and_events() {
    let dir = tempfile::tempdir().unwrap();
    let backend = Arc::new(LocalFsBackend::new(dir.path()));
    let run_repo = backend.clone() as Arc<dyn RunRepo>;
    let event_repo = backend.clone() as Arc<dyn RunEventRepo>;

    let plan = test_plan();
    let envelopes = execute_run(&run_repo, &event_repo, &plan).await.unwrap();

    assert!(!envelopes.is_empty());
    assert_eq!(envelopes[0].event_name, "user.input");
    assert_eq!(envelopes[0].session_id, "s01");

    // Verify run was persisted
    let runs = run_repo
        .list_runs_by_session("u01", "a01", "s01")
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);

    // Verify events were persisted
    let events = event_repo
        .list_events_by_run("u01", "a01", "s01", &runs[0].run_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
}
