//! Test helpers for building Session.

use std::collections::HashMap;
use std::sync::Arc;

use bendclaw::kernel::session::workspace::SandboxResolver;
use bendclaw::kernel::session::workspace::Workspace;
use bendclaw::kernel::tools::ToolContext;
use bendclaw::storage::Pool;

/// Build a test Workspace for a temp directory.
pub fn test_workspace(dir: std::path::PathBuf) -> Arc<Workspace> {
    Arc::new(Workspace::new(
        dir,
        vec!["PATH".into(), "HOME".into()],
        HashMap::new(),
        std::time::Duration::from_secs(5),
        1_048_576,
        Arc::new(SandboxResolver),
    ))
}

/// Create a dummy Pool that points to a non-existent endpoint.
/// Suitable for tests that never actually query the database.
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
        pool: dummy_pool(),
        is_dispatched: false,
    }
}
