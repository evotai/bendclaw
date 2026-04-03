use crate::observability::log::slog;

pub fn repo_error(
    repo: &str,
    action: &str,
    payload: serde_json::Value,
    error: &impl std::fmt::Display,
) {
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    slog!(error, "storage", "failed",
        repo,
        action,
        error = %error,
        payload = %payload_str,
    );
}
