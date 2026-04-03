//! Memory diagnostics — unified logging for the memory module.

use crate::observability::log::slog;

pub(crate) fn log_extract_started(user_id: &str, agent_id: &str) {
    slog!(
        info,
        "memory",
        "extract_started",
        user_id = user_id,
        agent_id = agent_id,
    );
}

pub(crate) fn log_extract_done(user_id: &str, facts_written: usize) {
    slog!(
        info,
        "memory",
        "extract_done",
        user_id = user_id,
        facts_written = facts_written,
    );
}

pub(crate) fn log_recall(user_id: &str, agent_id: &str, entries: usize, chars: usize) {
    slog!(
        info,
        "memory",
        "recall",
        user_id = user_id,
        agent_id = agent_id,
        entries = entries,
        chars = chars,
    );
}

pub(crate) fn log_hygiene(user_id: &str, pruned: usize) {
    slog!(
        info,
        "memory",
        "hygiene",
        user_id = user_id,
        pruned = pruned,
    );
}

pub(crate) fn log_save(user_id: &str, agent_id: &str, key: &str, scope: &str) {
    slog!(
        info,
        "memory",
        "save",
        user_id = user_id,
        agent_id = agent_id,
        key = key,
        scope = scope,
    );
}
