#[path = "tools/ask_user.rs"]
mod ask_user;
#[path = "tools/bash.rs"]
mod bash;
#[path = "tools/concurrency.rs"]
mod concurrency;
#[path = "tools/disallow.rs"]
mod disallow;
#[path = "tools/edit/mod.rs"]
mod edit;
#[path = "tools/file.rs"]
mod file;
#[path = "tools/guard.rs"]
mod guard;
#[path = "tools/list.rs"]
mod list;
#[path = "tools/memory.rs"]
mod memory;
#[path = "tools/search.rs"]
mod search;
#[path = "tools/skill.rs"]
mod skill;
#[path = "tools/tool_sets.rs"]
mod tool_sets;
#[path = "tools/validation.rs"]
mod validation;
#[path = "tools/web_fetch.rs"]
mod web_fetch;

use std::sync::Arc;

use evotengine::types::*;
use tokio_util::sync::CancellationToken;

/// Helper to build a ToolContext for tests.
pub fn ctx(name: &str) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: name.into(),
        cancel: CancellationToken::new(),
        on_update: None,
        on_progress: None,
        cwd: std::path::PathBuf::new(),
        path_guard: Arc::new(evotengine::PathGuard::open()),
    }
}

pub fn ctx_with_cancel(name: &str, cancel: CancellationToken) -> ToolContext {
    ToolContext {
        tool_call_id: "t1".into(),
        tool_name: name.into(),
        cancel,
        on_update: None,
        on_progress: None,
        cwd: std::path::PathBuf::new(),
        path_guard: Arc::new(evotengine::PathGuard::open()),
    }
}
