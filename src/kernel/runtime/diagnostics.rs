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
