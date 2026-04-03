use bendclaw::storage::agents::Agent;
use bendclaw::storage::agents::AgentRepo;
use bendclaw::storage::channels::Channel;
use bendclaw::storage::channels::ChannelRepo;
use bendclaw::storage::kind::StorageKind;
use bendclaw::storage::local_fs::LocalFsBackend;
use bendclaw::storage::run_events::RunEvent;
use bendclaw::storage::run_events::RunEventKind;
use bendclaw::storage::run_events::RunEventRepo;
use bendclaw::storage::runs::entity::Run;
use bendclaw::storage::runs::RunRepo;
use bendclaw::storage::sessions::Session;
use bendclaw::storage::sessions::SessionRepo;
use bendclaw::storage::skills::Skill;
use bendclaw::storage::skills::SkillRepo;
use bendclaw::storage::storage_backend::StorageBackend;
use bendclaw::storage::task_history::TaskHistory;
use bendclaw::storage::task_history::TaskHistoryRepo;
use bendclaw::storage::tasks::Task;
use bendclaw::storage::tasks::TaskRepo;
use bendclaw::storage::traces::Span;
use bendclaw::storage::traces::SpanRepo;
use bendclaw::storage::traces::Trace;
use bendclaw::storage::traces::TraceRepo;

fn ts() -> String {
    "2026-01-01T00:00:00Z".to_string()
}

#[test]
fn storage_kind_parse() {
    assert_eq!(StorageKind::parse("local").unwrap(), StorageKind::Local);
    assert_eq!(StorageKind::parse("cloud").unwrap(), StorageKind::Cloud);
    assert!(StorageKind::parse("unknown").is_err());
}

#[test]
fn storage_kind_display() {
    assert_eq!(StorageKind::Local.as_str(), "local");
    assert_eq!(StorageKind::Cloud.as_str(), "cloud");
    assert_eq!(format!("{}", StorageKind::Local), "local");
}

#[test]
fn local_fs_backend_kind() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());
    assert_eq!(backend.kind(), StorageKind::Local);
}

#[tokio::test]
async fn agent_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let agent = Agent {
        agent_id: "a01".into(),
        user_id: "u01".into(),
        name: "test".into(),
        model: "claude-3".into(),
        config: serde_json::Value::Null,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_agent(&agent).await.unwrap();

    let loaded = backend.get_agent("u01", "a01").await.unwrap();
    assert!(loaded.is_some());
    assert_eq!(loaded.unwrap().name, "test");

    let agents = backend.list_agents("u01").await.unwrap();
    assert_eq!(agents.len(), 1);

    backend.delete_agent("u01", "a01").await.unwrap();
    assert!(backend.get_agent("u01", "a01").await.unwrap().is_none());
}

#[tokio::test]
async fn session_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let session = Session {
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        title: "test session".into(),
        scope: String::new(),
        state: serde_json::Value::Null,
        meta: serde_json::Value::Null,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.create_session(&session).await.unwrap();

    let found = backend.find_session("u01", "a01", "s01").await.unwrap();
    assert!(found.is_some());
    assert_eq!(found.unwrap().title, "test session");

    let latest = backend.find_latest_session("u01", "a01").await.unwrap();
    assert!(latest.is_some());
}

#[tokio::test]
async fn run_and_handoff_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let run = Run {
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_run_id: String::new(),
        root_trace_id: "t01".into(),
        kind: "user_turn".into(),
        status: "RUNNING".into(),
        input: serde_json::Value::Null,
        output: serde_json::Value::Null,
        error: serde_json::Value::Null,
        metrics: serde_json::Value::Null,
        stop_reason: String::new(),
        iterations: 0,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_run(&run).await.unwrap();

    let loaded = backend.get_run("u01", "a01", "s01", "r01").await.unwrap();
    assert!(loaded.is_some());

    let incomplete = backend.list_incomplete_runs("u01", "a01").await.unwrap();
    assert_eq!(incomplete.len(), 1);

    let handoff = serde_json::json!({"turn": 3, "pending_tools": []});
    backend
        .save_handoff("u01", "a01", "s01", "r01", &handoff)
        .await
        .unwrap();
    let loaded_handoff = backend
        .load_handoff("u01", "a01", "s01", "r01")
        .await
        .unwrap();
    assert_eq!(loaded_handoff.unwrap()["turn"], 3);

    backend
        .clear_handoff("u01", "a01", "s01", "r01")
        .await
        .unwrap();
    assert!(backend
        .load_handoff("u01", "a01", "s01", "r01")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn run_event_append_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let evt1 = RunEvent {
        event_id: "e01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        seq: 1,
        kind: RunEventKind::UserInput,
        payload: serde_json::json!({"text": "hello"}),
        created_at: ts(),
    };
    let evt2 = RunEvent {
        event_id: "e02".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        seq: 2,
        kind: RunEventKind::AssistantOutput,
        payload: serde_json::json!({"text": "hi"}),
        created_at: ts(),
    };
    backend.append_event(&evt1).await.unwrap();
    backend.append_event(&evt2).await.unwrap();

    let events = backend
        .list_events_by_run("u01", "a01", "s01", "r01")
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].kind, RunEventKind::UserInput);
    assert_eq!(events[1].kind, RunEventKind::AssistantOutput);
}

#[tokio::test]
async fn trace_and_span_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let trace = Trace {
        trace_id: "t01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_trace_id: String::new(),
        name: "root".into(),
        status: "ok".into(),
        created_at: ts(),
        updated_at: ts(),
        doc: serde_json::json!({"duration_ms": 100}),
    };
    backend.save_trace(&trace).await.unwrap();

    let loaded = backend.get_trace("u01", "a01", "t01").await.unwrap();
    assert!(loaded.is_some());

    let span = Span {
        span_id: "sp01".into(),
        trace_id: "t01".into(),
        run_id: "r01".into(),
        session_id: "s01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        parent_span_id: String::new(),
        name: "llm_call".into(),
        kind: "llm".into(),
        status: "ok".into(),
        created_at: ts(),
        doc: serde_json::json!({"duration_ms": 50}),
    };
    backend.append_span(&span).await.unwrap();

    let spans = backend
        .list_spans_by_trace("u01", "a01", "t01")
        .await
        .unwrap();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].span_id, "sp01");
}

#[tokio::test]
async fn task_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let task = Task {
        task_id: "tk01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        name: "daily".into(),
        prompt: "report".into(),
        enabled: true,
        status: "active".into(),
        schedule: serde_json::Value::Null,
        delivery: serde_json::Value::Null,
        scope: String::new(),
        created_by: String::new(),
        delete_after_run: false,
        run_count: 0,
        last_error: None,
        last_run_at: String::new(),
        next_run_at: None,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_task(&task).await.unwrap();

    let loaded = backend.get_task("u01", "a01", "tk01").await.unwrap();
    assert!(loaded.is_some());

    let tasks = backend.list_tasks("u01", "a01").await.unwrap();
    assert_eq!(tasks.len(), 1);

    backend.delete_task("u01", "a01", "tk01").await.unwrap();
    assert!(backend
        .get_task("u01", "a01", "tk01")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn task_history_append_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let entry = TaskHistory {
        history_id: "h01".into(),
        task_id: "tk01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        run_id: Some("r01".into()),
        task_name: "daily".into(),
        schedule: serde_json::Value::Null,
        prompt: "report".into(),
        status: "completed".into(),
        output: Some("done".into()),
        error: None,
        duration_ms: Some(100),
        delivery: serde_json::Value::Null,
        delivery_status: None,
        delivery_error: None,
        executed_by_node_id: None,
        created_at: ts(),
    };
    backend.append_history(&entry).await.unwrap();

    let history = backend
        .list_history_by_task("u01", "a01", "tk01")
        .await
        .unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].history_id, "h01");
}

#[tokio::test]
async fn skill_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let skill = Skill {
        skill_id: "sk01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        name: "test-skill".into(),
        source: "local".into(),
        manifest: serde_json::Value::Null,
        enabled: true,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_skill(&skill).await.unwrap();

    let loaded = backend.get_skill("u01", "a01", "sk01").await.unwrap();
    assert!(loaded.is_some());

    let skills = backend.list_skills("u01", "a01").await.unwrap();
    assert_eq!(skills.len(), 1);

    backend.delete_skill("u01", "a01", "sk01").await.unwrap();
    assert!(backend
        .get_skill("u01", "a01", "sk01")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn channel_roundtrip() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let channel = Channel {
        channel_id: "ch01".into(),
        agent_id: "a01".into(),
        user_id: "u01".into(),
        kind: "telegram".into(),
        config: serde_json::Value::Null,
        status: "active".into(),
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_channel(&channel).await.unwrap();

    let loaded = backend.get_channel("u01", "a01", "ch01").await.unwrap();
    assert!(loaded.is_some());

    let channels = backend.list_channels("u01", "a01").await.unwrap();
    assert_eq!(channels.len(), 1);

    backend.delete_channel("u01", "a01", "ch01").await.unwrap();
    assert!(backend
        .get_channel("u01", "a01", "ch01")
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn local_fs_directory_layout() {
    let dir = tempfile::tempdir().unwrap();
    let backend = LocalFsBackend::new(dir.path());

    let agent = Agent {
        agent_id: "myagent".into(),
        user_id: "myuser".into(),
        name: "test".into(),
        model: "claude-3".into(),
        config: serde_json::Value::Null,
        created_at: ts(),
        updated_at: ts(),
    };
    backend.save_agent(&agent).await.unwrap();

    // Verify the directory layout matches spec:
    // <root>/users/<user_id>/agents/<agent_id>/agent.json
    let agent_path = dir
        .path()
        .join("users")
        .join("myuser")
        .join("agents")
        .join("myagent")
        .join("agent.json");
    assert!(
        agent_path.exists(),
        "agent.json should be at {agent_path:?}"
    );
}
