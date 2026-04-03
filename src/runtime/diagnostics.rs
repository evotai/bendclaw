pub(super) fn log_runtime_invalidated(agent_id: &str, evicted_idle: usize, marked_running: usize) {
    crate::observability::log::slog!(
        info,
        "runtime",
        "invalidated",
        agent_id,
        evicted_idle,
        marked_running,
    );
}

pub(super) fn log_runtime_command_completed(
    command: &str,
    status: &str,
    agent_id: &str,
    elapsed_ms: u64,
    payload: &str,
) {
    crate::observability::log::slog!(
        info,
        "runtime",
        "completed",
        command,
        status,
        agent_id,
        elapsed_ms,
        payload = %payload,
    );
}

pub(super) fn log_runtime_command_failed(
    command: &str,
    agent_id: &str,
    elapsed_ms: u64,
    error: &impl std::fmt::Display,
    payload: &str,
) {
    crate::observability::log::slog!(
        error,
        "runtime",
        "failed",
        command,
        agent_id,
        elapsed_ms,
        error = %error,
        payload = %payload,
    );
}

pub(crate) fn log_runtime_denied(agent_id: &str, user_id: &str, session_id: &str) {
    crate::observability::log::slog!(error, "runtime", "denied", agent_id, user_id, session_id,);
}

pub(crate) fn log_runtime_recreated(agent_id: &str, user_id: &str, session_id: &str) {
    crate::observability::log::slog!(
        info,
        "runtime",
        "recreated",
        reason = "stale_llm_config",
        agent_id,
        user_id,
        session_id,
    );
}

pub(crate) fn log_runtime_reused(agent_id: &str, user_id: &str, session_id: &str) {
    crate::observability::log::slog!(info, "runtime", "reused", agent_id, user_id, session_id,);
}

pub(crate) fn log_runtime_session_created(
    agent_id: &str,
    user_id: &str,
    session_id: &str,
    workspace_dir: &str,
    tool_count: usize,
) {
    crate::observability::log::slog!(
        info,
        "runtime",
        "created",
        agent_id,
        user_id,
        session_id,
        workspace_dir = %workspace_dir,
        tool_count,
    );
}

pub(super) fn log_runtime_directive_init_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "runtime", "directive_init_failed", error = %error,);
}

pub(super) fn log_runtime_cluster_init_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "runtime", "cluster_init_failed", error = %error,);
}

pub(super) fn log_runtime_flush_failed(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "runtime", "flush_failed", agent_id = %agent_id, error = %error,);
}

pub(super) fn log_runtime_shutdown_timeout() {
    crate::observability::log::slog!(warn, "runtime", "shutdown_timeout",);
}

pub(super) fn log_runtime_stopped(elapsed_ms: u64) {
    crate::observability::log::slog!(info, "runtime", "stopped", elapsed_ms,);
}

pub(super) struct ControlCommandInfo<'a> {
    pub agent_id: &'a str,
    pub user_id: &'a str,
    pub session_id: &'a str,
    pub input: &'a str,
    pub normalized: &'a str,
    pub command: &'a str,
    pub handled: bool,
    pub handler: &'a str,
}

pub(super) fn log_control_command_classified(info: ControlCommandInfo<'_>) {
    crate::observability::log::slog!(
        info,
        "runtime",
        "control_command_classified",
        agent_id = %info.agent_id,
        user_id = %info.user_id,
        session_id = %info.session_id,
        input_preview = %crate::observability::server_log::preview_text(info.input),
        normalized_input = %info.normalized,
        command = %info.command,
        handled = info.handled,
        handler = %info.handler,
    );
}
