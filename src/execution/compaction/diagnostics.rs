pub(crate) fn log_compaction_skipped(failures: u32, last_error: &str) {
    crate::observability::log::slog!(warn, "compaction", "skipped", failures, last_error,);
}

pub(crate) fn log_compaction_triggered(total_tokens: usize, max_context_tokens: usize) {
    crate::observability::log::slog!(
        info,
        "compaction",
        "triggered",
        total_tokens,
        max_context_tokens,
    );
}

pub(crate) fn log_compaction_cooldown_active(elapsed_secs: u64, failures: u32) {
    crate::observability::log::slog!(
        info,
        "compaction",
        "cooldown_active",
        elapsed_secs,
        failures,
    );
}

pub(crate) fn log_compaction_ineffective(
    messages_before: usize,
    messages_after: usize,
    consecutive_failures: u32,
) {
    crate::observability::log::slog!(
        warn,
        "compaction",
        "ineffective",
        messages_before,
        messages_after,
        consecutive_failures,
    );
}

pub(crate) fn log_compaction_tokens_barely_reduced(pre_tokens: usize, post_tokens: usize) {
    crate::observability::log::slog!(
        warn,
        "compaction",
        "tokens_barely_reduced",
        pre_tokens,
        post_tokens,
    );
}

pub(crate) fn log_compaction_summarize_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "compaction", "summarize_failed", error = %error,);
}
