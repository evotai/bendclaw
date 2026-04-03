pub(crate) fn log_skill_args_parse_failed(skill: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "skill", "args_parse_failed", skill = skill, error = %error,);
}

pub(crate) fn log_skill_failed(skill: &str, latency_ms: u64, exit_code: i32, stderr: &str) {
    crate::observability::log::slog!(
        warn,
        "skill",
        "failed",
        skill = skill,
        latency_ms,
        exit_code,
        stderr = %stderr,
    );
}

pub(crate) fn log_skill_completed(skill: &str, latency_ms: u64, exit_code: i32, stdout_len: usize) {
    crate::observability::log::slog!(
        info,
        "skill",
        "completed",
        skill = skill,
        latency_ms,
        exit_code,
        stdout_len,
    );
}

pub(crate) fn log_skill_dir_read_failed(dir: &std::path::Path, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "skill", "dir_read_failed", dir = %dir.display(), error = %error,);
}

pub(crate) fn log_skill_sanitizer_detected(pattern: &str, description: &str, needle: &str) {
    crate::observability::log::slog!(
        info,
        "skill",
        "sanitizer_detected",
        pattern,
        description,
        needle,
    );
}

pub(crate) fn log_skill_sanitizer_replaced(pattern: &str, occurrences: u32) {
    crate::observability::log::slog!(info, "skill", "sanitizer_replaced", pattern, occurrences,);
}

pub(crate) fn log_skill_unsafe_path(skill: &str, path: &str) {
    crate::observability::log::slog!(warn, "skill", "unsafe_path", skill = %skill, path = %path,);
}

pub(crate) fn log_skill_hub_clone_failed(stderr: &str) {
    crate::observability::log::slog!(warn, "skill", "hub_clone_failed", stderr = %stderr,);
}

pub(crate) fn log_skill_sync_list_failed(agent_id: &str, error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "skill_sync", "skill_list_failed", agent_id = %agent_id, error = %error,);
}

pub(crate) fn log_skill_sync_failed(error: &impl std::fmt::Display, consecutive_errors: u64) {
    crate::observability::log::slog!(error, "skill_sync", "failed", error = %error, consecutive_errors,);
}
