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
