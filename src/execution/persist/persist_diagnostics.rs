use crate::observability::log::slog;

pub(crate) fn log_session_upsert_failed(error: &impl std::fmt::Display) {
    slog!(warn, "persist", "session_upsert_failed", error = %error,);
}

pub(crate) fn log_session_mark_replaced_failed(
    session_id: &str,
    replaced_by_session_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(warn, "persist", "session_mark_replaced_failed",
        session_id = %session_id,
        replaced_by_session_id = %replaced_by_session_id,
        error = %error,
    );
}

pub(crate) fn log_session_delete_failed(session_id: &str, error: &impl std::fmt::Display) {
    slog!(warn, "persist", "session_delete_failed",
        session_id = %session_id,
        error = %error,
    );
}

pub(crate) fn log_run_session_upsert_failed(
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(warn, "persist", "session_upsert_failed",
        run_id = %run_id,
        session_id = %session_id,
        agent_id = %agent_id,
        error = %error,
    );
}

pub(crate) fn log_run_insert_failed(
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(warn, "persist", "run_insert_failed",
        run_id = %run_id,
        session_id = %session_id,
        agent_id = %agent_id,
        error = %error,
    );
}

pub(crate) fn log_usage_failed(
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(warn, "persist", "usage_failed",
        run_id = %run_id,
        session_id = %session_id,
        agent_id = %agent_id,
        error = %error,
    );
}

pub(crate) fn log_run_events_failed(
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(error, "persist", "run_events_failed",
        run_id = %run_id,
        session_id = %session_id,
        agent_id = %agent_id,
        error = %error,
    );
}

pub(crate) fn log_run_update_failed(
    level: &str,
    run_id: &str,
    session_id: &str,
    agent_id: &str,
    error: &impl std::fmt::Display,
) {
    match level {
        "warn" => slog!(warn, "persist", "run_update_failed",
            run_id = %run_id,
            session_id = %session_id,
            agent_id = %agent_id,
            error = %error,
        ),
        _ => slog!(error, "persist", "run_update_failed",
            run_id = %run_id,
            session_id = %session_id,
            agent_id = %agent_id,
            error = %error,
        ),
    }
}

pub(crate) fn log_cancel_event_failed(run_id: &str, error: &impl std::fmt::Display) {
    slog!(error, "persist", "cancel_event_failed",
        run_id = %run_id,
        error = %error,
    );
}

pub(crate) fn log_cancel_status_failed(run_id: &str, error: &impl std::fmt::Display) {
    slog!(warn, "persist", "cancel_status_failed",
        run_id = %run_id,
        error = %error,
    );
}

pub(crate) fn log_checkpoint_insert_failed(
    run_id: &str,
    session_id: &str,
    error: &impl std::fmt::Display,
) {
    slog!(warn, "persist", "checkpoint_insert_failed",
        run_id = %run_id,
        session_id = %session_id,
        error = %error,
    );
}
