use std::path::Path;
use std::process::Command;

fn rg(pattern: &str, dir: &str) -> Vec<String> {
    let output = Command::new("rg")
        .args(["-l", pattern, dir])
        .output()
        .expect("rg must be available");
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect()
}

fn rg_count(pattern: &str, dir: &str) -> usize {
    rg(pattern, dir).len()
}

fn src_dir() -> String {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into());
    format!("{manifest}/src")
}

// ── storage.type isolation ────────────────────────────────────────────────────

#[test]
fn storage_type_only_in_startup_and_backend() {
    let src = src_dir();
    let files = rg("StorageKind", &src);
    for f in &files {
        let f = f.replace(&src, "");
        assert!(
            f.contains("/bin/")
                || f.contains("/storage/backend/")
                || f.contains("/storage/kind.rs")
                || f.contains("/storage/local_fs.rs")
                || f.contains("/storage/databend_backend.rs")
                || f.contains("/storage/storage_backend.rs")
                || f.contains("/config"),
            "StorageKind referenced outside startup/backend: {f}"
        );
    }
}

// ── entity ownership scopes ──────────────────────────────────────────────────

#[test]
fn entity_run_has_session_id_field() {
    let src = src_dir();
    let content =
        std::fs::read_to_string(format!("{src}/types/entities/run.rs")).expect("run.rs exists");
    assert!(
        content.contains("pub session_id: String"),
        "Run must have session_id"
    );
    assert!(
        content.contains("pub agent_id: String"),
        "Run must have agent_id"
    );
    assert!(
        content.contains("pub user_id: String"),
        "Run must have user_id"
    );
}

#[test]
fn entity_span_has_all_ancestor_ids() {
    let src = src_dir();
    let content =
        std::fs::read_to_string(format!("{src}/types/entities/span.rs")).expect("span.rs exists");
    for field in ["user_id", "agent_id", "session_id", "run_id", "trace_id"] {
        assert!(
            content.contains(&format!("pub {field}: String")),
            "Span must have {field}"
        );
    }
}

#[test]
fn entity_task_has_agent_id() {
    let src = src_dir();
    let content =
        std::fs::read_to_string(format!("{src}/types/entities/task.rs")).expect("task.rs exists");
    assert!(
        content.contains("pub agent_id: String"),
        "Task must have agent_id"
    );
    assert!(
        content.contains("pub user_id: String"),
        "Task must have user_id"
    );
}

#[test]
fn entity_task_history_has_agent_id() {
    let src = src_dir();
    let content = std::fs::read_to_string(format!("{src}/types/entities/task_history.rs"))
        .expect("task_history.rs exists");
    assert!(
        content.contains("pub agent_id: String"),
        "TaskHistory must have agent_id"
    );
}

// ── handoff model separation ─────────────────────────────────────────────────

#[test]
fn four_continuity_models_are_separate_files() {
    let src = src_dir();
    assert!(
        Path::new(&format!("{src}/kernel/session/core/session_rules.rs")).exists(),
        "session_rules.rs must exist"
    );
    assert!(
        Path::new(&format!("{src}/kernel/session/core/session_memory.rs")).exists(),
        "session_memory.rs must exist"
    );
    assert!(
        Path::new(&format!("{src}/kernel/run/persist/run_handoff.rs")).exists(),
        "run_handoff.rs must exist"
    );
    assert!(
        Path::new(&format!("{src}/kernel/run/persist/run_cleanup.rs")).exists(),
        "run_cleanup.rs must exist"
    );
}

// ── no bendclaw-local ────────────────────────────────────────────────────────

#[test]
fn no_bendclaw_local_binary() {
    let src = src_dir();
    assert!(
        !Path::new(&format!("{src}/bin/bendclaw-local.rs")).exists(),
        "bendclaw-local.rs must not exist"
    );
}

#[test]
fn no_local_module() {
    let src = src_dir();
    assert!(
        !Path::new(&format!("{src}/local")).exists(),
        "src/local/ must not exist"
    );
}

// ── app/result is sole formatting authority ───────────────────────────────────

#[test]
fn cli_output_deleted() {
    let src = src_dir();
    // cli/output.rs should still exist during transition (it's a thin wrapper),
    // but it must not contain SSE/stream-json logic
    if Path::new(&format!("{src}/cli/output.rs")).exists() {
        let content = std::fs::read_to_string(format!("{src}/cli/output.rs")).unwrap_or_default();
        assert!(
            !content.contains("SSE") && !content.contains("stream_json"),
            "cli/output.rs must not contain SSE or stream_json formatting"
        );
    }
}

// ── run_event extensibility ──────────────────────────────────────────────────

#[test]
fn run_event_kind_has_custom_variant() {
    let src = src_dir();
    let content = std::fs::read_to_string(format!("{src}/types/entities/run_event.rs"))
        .expect("run_event.rs exists");
    assert!(
        content.contains("Custom(String)"),
        "RunEventKind must have Custom(String) variant for extensibility"
    );
}

// ── storage backend narrow traits ────────────────────────────────────────────

#[test]
fn all_entity_repos_exist() {
    let src = src_dir();
    let repo_paths = [
        "storage/agents/agent_repo.rs",
        "storage/skills/skill_repo.rs",
        "storage/channels/channel_repo.rs",
        "storage/sessions/session_repo.rs",
        "storage/runs/run_repo.rs",
        "storage/run_events/run_event_repo.rs",
        "storage/traces/trace_repo.rs",
        "storage/traces/span_repo.rs",
        "storage/tasks/task_repo.rs",
        "storage/task_history/task_history_repo.rs",
    ];
    for repo in repo_paths {
        assert!(
            Path::new(&format!("{src}/{repo}")).exists(),
            "narrow repo trait {repo} must exist"
        );
    }
}

// ── local directory layout ───────────────────────────────────────────────────

#[test]
fn local_fs_backend_uses_user_agent_path() {
    let src = src_dir();
    let content =
        std::fs::read_to_string(format!("{src}/storage/local_fs.rs")).expect("local_fs.rs exists");
    assert!(
        content.contains(r#""users""#) && content.contains(r#""agents""#),
        "LocalFsBackend must use users/<user_id>/agents/<agent_id> path"
    );
}
