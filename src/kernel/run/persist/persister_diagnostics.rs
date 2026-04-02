pub(crate) fn log_run_failed(
    agent_id: &str,
    session_id: &str,
    run_id: &str,
    elapsed_ms: u64,
    error: &str,
) {
    crate::observability::log::slog!(
        error,
        "run",
        "failed",
        agent_id = %agent_id,
        session_id = %session_id,
        run_id = %run_id,
        elapsed_ms,
        error = %error,
    );
}
