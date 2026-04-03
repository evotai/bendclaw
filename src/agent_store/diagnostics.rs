pub(crate) fn log_usage_flush_retry(error: &impl std::fmt::Display, count: usize, attempt: usize) {
    crate::observability::log::slog!(
        warn,
        "usage",
        "flush_retry",
        error = %error,
        count,
        attempt,
    );
}

pub(crate) fn log_usage_flush_requeued(count: usize) {
    crate::observability::log::slog!(warn, "usage", "flush_requeued", count,);
}
