pub(crate) fn log_task_executed(
    agent_id: &str,
    task_id: &str,
    status: &str,
    duration_ms: i32,
    delivery_status: &str,
    delivery_error: &str,
) {
    crate::observability::log::slog!(
        info,
        "task",
        "executed",
        agent_id,
        task_id,
        status,
        duration_ms,
        delivery_status = %delivery_status,
        delivery_error = %delivery_error,
    );
}

pub(crate) fn log_channel_context_unavailable(
    agent_id: &str,
    channel_account_id: &str,
    chat_id: &str,
) {
    crate::observability::log::slog!(
        warn,
        "task",
        "channel_context_unavailable",
        agent_id,
        channel_account_id,
        chat_id,
    );
}

pub(crate) fn log_task_discover_skip(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "task", "discover_skip", agent_id, error = %error,);
}

pub(crate) fn log_task_list_failed(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "task", "list_failed", agent_id, error = %error,);
}

pub(crate) fn log_task_execution_failed(
    task_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(
        error,
        "task",
        "execution_failed",
        task_id,
        agent_id,
        error = %error,
    );
}

pub(crate) fn log_task_execution_timeout(task_id: &str, agent_id: &str, timeout_secs: u64) {
    crate::observability::log::slog!(
        error,
        "task",
        "execution_timeout",
        task_id,
        agent_id,
        timeout_secs,
    );
}

pub(crate) fn log_task_reset_status_failed(task_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(
        warn,
        "task",
        "reset_status_failed",
        task_id = %task_id,
        error = %error,
    );
}

pub(crate) fn log_task_history_failed(task_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(error, "task", "history_failed", task_id, error = %error,);
}
