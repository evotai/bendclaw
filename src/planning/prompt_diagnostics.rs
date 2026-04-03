pub(crate) fn log_prompt_layer_truncated(
    layer: &str,
    original_size: usize,
    truncated_size: usize,
    dropped_bytes: usize,
    max: usize,
    source: &str,
) {
    crate::observability::log::slog!(
        warn,
        "prompt",
        "layer_truncated",
        layer,
        original_size,
        truncated_size,
        dropped_bytes,
        max,
        source,
    );
}

pub(crate) fn log_prompt_variables_db_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "prompt", "variables_db_failed", error = %error,);
}

pub(crate) fn log_prompt_recent_errors_db_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "prompt", "recent_errors_db_failed", error = %error,);
}
