pub(crate) fn log_refresh(prompt: Option<&str>, changed: bool, elapsed_ms: u64) {
    match (prompt, changed) {
        (Some(text), true) => crate::observability::log::slog!(
            info,
            "directive",
            "refreshed",
            size = text.len(),
            elapsed_ms,
        ),
        (Some(text), false) => crate::observability::log::slog!(
            info,
            "directive",
            "unchanged",
            size = text.len(),
            elapsed_ms,
        ),
        (None, true) => crate::observability::log::slog!(info, "directive", "cleared", elapsed_ms,),
        (None, false) => crate::observability::log::slog!(info, "directive", "empty", elapsed_ms,),
    }
}

pub(crate) fn log_loop_started(refresh_interval_ms: u64) {
    crate::observability::log::slog!(info, "directive", "loop_started", refresh_interval_ms,);
}

pub(crate) fn log_refresh_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "directive", "refresh_failed", error = %error,);
}
