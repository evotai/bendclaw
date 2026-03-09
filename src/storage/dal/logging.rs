pub fn repo_error(
    repo: &str,
    action: &str,
    payload: serde_json::Value,
    error: &impl std::fmt::Display,
) {
    let payload_str = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
    tracing::error!(
        log_kind = "server_log",
        stage = "storage_repo",
        repo,
        action,
        status = "failed",
        error = %error,
        payload = %payload_str,
        "storage repo"
    );
}
