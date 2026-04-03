pub(crate) fn log_trace_insert_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "trace", "insert_failed", error = %error,);
}

pub(crate) fn log_trace_update_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "trace", "update_failed", error = %error,);
}

pub(crate) fn log_trace_append_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "trace", "append_failed", error = %error,);
}
