//! Session-scoped reclaim accessors.
//!
//! These resolve a session id to its process manager. A wrong or missing lookup
//! returns zero rather than failing, which in the TUI means esc finds "nothing
//! waiting" and falls straight through to killing the run — the exact outcome
//! the reclaim gesture exists to avoid. So the lookup itself is worth pinning.

use std::sync::Arc;

use evot::agent::Agent;
use evot::agent::BackgroundReason;
use evot::agent::QueryRequest;
use evot::agent::SubmitOutcome;
use evot::agent::ToolMode;
use evot::conf::Config;
use evot::conf::Protocol;
use evot::conf::ProviderProfile;
use evot::storage::MemoryStorage;
use evot_engine::provider::mock::MockToolCall;
use evot_engine::provider::MockProvider;
use evot_engine::provider::MockResponse;

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn provider_config(tmp: &tempfile::TempDir) -> Config {
    let mut config = Config::new(tmp.path().to_path_buf());
    config.providers.insert("test".into(), ProviderProfile {
        protocol: Protocol::OpenAi,
        api_key: "test-key".into(),
        base_url: "http://localhost".into(),
        models: vec!["test-model".into()],
        compat_caps: Default::default(),
        route_capabilities: Default::default(),
        thinking_level: None,
        context_window: None,
        max_tokens: None,
        supports_image: None,
    });
    config.llm.provider = "test".into();
    config
}

fn test_agent(tmp: &tempfile::TempDir) -> Result<Arc<Agent>, Box<dyn std::error::Error>> {
    let config = provider_config(tmp);
    Ok(Agent::new_with_provider_for_test(
        &config,
        tmp.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        MockProvider::text("done"),
    )?)
}

/// An agent whose first turn detaches a long shell, so the session provably owns
/// a process manager with live work in it.
fn agent_that_starts_a_background_shell(
    tmp: &tempfile::TempDir,
) -> Result<Arc<Agent>, Box<dyn std::error::Error>> {
    let config = provider_config(tmp);
    let provider = MockProvider::new(vec![
        MockResponse::ToolCalls(vec![MockToolCall {
            name: "bash".into(),
            arguments: serde_json::json!({
                "command": "sleep 30",
                "run_in_background": true,
            }),
        }]),
        MockResponse::Text("started".into()),
    ]);
    Ok(Agent::new_with_provider_for_test(
        &config,
        tmp.path().to_string_lossy(),
        Arc::new(MemoryStorage::new()),
        provider,
    )?)
}

/// Runs one interactive turn so the session acquires a process manager.
///
/// Interactive mode is the one that allows background processes, and the manager
/// is created lazily when a run is built — so a session that has never run has
/// nothing to look up.
async fn session_with_manager(agent: &Arc<Agent>) -> Result<String, Box<dyn std::error::Error>> {
    let meta = agent.create_session("test").await?;
    let session = agent
        .load_session(&meta.session_id)
        .await?
        .ok_or_else(|| std::io::Error::other("missing session"))?;
    let outcome = agent
        .submit_to_session(
            QueryRequest::text("hello").mode(ToolMode::Interactive),
            session,
        )
        .await?;
    if let SubmitOutcome::Run(mut run) = outcome {
        while run.next().await.is_some() {}
    }
    Ok(meta.session_id)
}

#[tokio::test]
async fn a_session_with_live_work_really_owns_a_manager() -> TestResult {
    // Anchors every other test in this file. They all assert zero, which is also
    // what a missing manager returns -- so without one case proving the lookup
    // resolves to a real registry, the whole file could pass while the accessors
    // were wired to nothing.
    let tmp = tempfile::TempDir::new()?;
    let agent = agent_that_starts_a_background_shell(&tmp)?;
    let session_id = session_with_manager(&agent).await?;

    let tasks = agent.background_processes(&session_id);
    assert_eq!(
        tasks.len(),
        1,
        "expected the detached shell, got: {tasks:?}"
    );

    // The shell detached itself, so nothing is in the foreground and no wait is
    // outstanding: this is the state the screenshot showed, where esc used to
    // find nothing to release and killed the run instead.
    assert_eq!(
        agent.background_foreground_processes(&session_id, BackgroundReason::UserRequested),
        0
    );
    assert_eq!(agent.blocking_task_waits(&session_id), 0);

    agent.stop_all_background_processes(&session_id).await;
    Ok(())
}

#[tokio::test]
async fn an_idle_session_has_nothing_to_reclaim() -> TestResult {
    let tmp = tempfile::TempDir::new()?;
    let agent = test_agent(&tmp)?;
    let session_id = session_with_manager(&agent).await?;

    // No shell and no wait: the caller is expected to escalate to interrupting.
    assert_eq!(
        agent.background_foreground_processes(&session_id, BackgroundReason::UserRequested),
        0
    );
    assert_eq!(agent.blocking_task_waits(&session_id), 0);
    assert_eq!(agent.release_blocking_task_waits(&session_id), 0);
    Ok(())
}

#[tokio::test]
async fn an_unknown_session_reports_nothing_rather_than_failing() -> TestResult {
    // Sessions disappear via /clear, /new and resume, and a keypress can land in
    // that window. Reporting zero is right; panicking or erroring is not.
    let tmp = tempfile::TempDir::new()?;
    let agent = test_agent(&tmp)?;

    assert_eq!(
        agent.background_foreground_processes("no-such-session", BackgroundReason::UserRequested),
        0
    );
    assert_eq!(agent.blocking_task_waits("no-such-session"), 0);
    assert_eq!(agent.release_blocking_task_waits("no-such-session"), 0);
    Ok(())
}

#[tokio::test]
async fn reclaim_accessors_are_scoped_to_one_session() -> TestResult {
    // The lookup is by session id, so one session's gesture must never disturb
    // another's work. Both sessions are idle here, so the assertion is that
    // neither call reaches across.
    let tmp = tempfile::TempDir::new()?;
    let agent = test_agent(&tmp)?;
    let first = session_with_manager(&agent).await?;
    let second = session_with_manager(&agent).await?;
    assert_ne!(first, second);

    assert_eq!(
        agent.background_foreground_processes(&first, BackgroundReason::UserRequested),
        0
    );
    assert_eq!(agent.blocking_task_waits(&second), 0);
    Ok(())
}

#[tokio::test]
async fn a_session_that_never_ran_has_no_manager() -> TestResult {
    // A created-but-unrun session has no process manager yet. The accessors must
    // treat that as "nothing waiting" rather than materialising one.
    let tmp = tempfile::TempDir::new()?;
    let agent = test_agent(&tmp)?;
    let meta = agent.create_session("test").await?;

    assert_eq!(agent.blocking_task_waits(&meta.session_id), 0);
    assert_eq!(
        agent.background_foreground_processes(&meta.session_id, BackgroundReason::MessageDelivery),
        0
    );
    assert!(agent.background_processes(&meta.session_id).is_empty());
    Ok(())
}
