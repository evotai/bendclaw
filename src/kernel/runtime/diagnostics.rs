pub(super) fn log_resolved_channel_session(
    base_key: &str,
    resolved_session_id: &str,
    source: &str,
) {
    crate::observability::log::slog!(info, "runtime", "session_resolved",
        msg = "channel session resolved",
        base_key,
        resolved_session_id = %resolved_session_id,
        source,
    );
}

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
