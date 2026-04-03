//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

use bendclaw::sessions::workspace::SandboxResolver;
use bendclaw::sessions::workspace::Workspace;
use bendclaw::storage::Pool;
use bendclaw::tools::ToolContext;

/// Build a test Workspace for a temp directory.
pub fn test_workspace(dir: std::path::PathBuf) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        dir.clone(),
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        std::time::Duration::from_secs(5),
        std::time::Duration::from_secs(300),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

/// Create a dummy Pool that points to a non-existent endpoint.
/// Suitable for tests that never actually query the database.
#[allow(dead_code)]
pub fn dummy_pool() -> Pool {
    Pool::new("http://localhost:0", "", "default").expect("dummy pool: invalid URL is unreachable")
}

/// Build a test `Session` with tools wired up.
pub fn test_tool_context() -> ToolContext {
    use ulid::Ulid;
    let dir = std::env::temp_dir().join(format!("bendclaw-test-ctx-{}", Ulid::new()));
    let _ = std::fs::create_dir_all(&dir);
    ToolContext {
        user_id: format!("u-{}", Ulid::new()).into(),
        session_id: format!("s-{}", Ulid::new()).into(),
        agent_id: "a1".into(),
        run_id: "r-test".into(),
        trace_id: "t-test".into(),
        workspace: test_workspace(dir),
        is_dispatched: false,
        runtime: bendclaw::tools::ToolRuntime {
            event_tx: None,
            cancel: tokio_util::sync::CancellationToken::new(),
            tool_call_id: None,
        },
        tool_writer: bendclaw::writer::BackgroundWriter::noop("tool_write"),
    }
}
