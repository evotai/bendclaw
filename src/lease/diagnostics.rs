pub(crate) fn log_lease_release_failed(
    table: &str,
    resource_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(warn, "lease", "release_failed", table, resource_id = %resource_id, error = %error,);
}

pub(crate) fn log_lease_scan_recovered(table: &str, consecutive_errors: u64) {
    crate::observability::log::slog!(
        info,
        "lease",
        "scan_recovered",
        table = table,
        consecutive_errors,
    );
}

pub(crate) fn log_lease_scan_error(
    table: &str,
    error: &impl std::fmt::Display,
    consecutive_errors: u64,
) {
    crate::observability::log::slog!(warn, "lease", "scan_error", table = table, error = %error, consecutive_errors,);
}

pub(crate) fn log_lease_resources_discovered(
    table: &str,
    count: u64,
    discover_ms: u64,
    prev_count: u64,
) {
    if count != prev_count {
        crate::observability::log::slog!(
            info,
            "lease",
            "resources_discovered",
            table = table,
            count,
            discover_ms,
        );
    }
}

pub(crate) fn log_lease_unhealthy_released(table: &str, resource_id: &str) {
    crate::observability::log::slog!(warn, "lease", "unhealthy_released", table, resource_id = %resource_id,);
}

pub(crate) fn log_lease_renew_failed(
    table: &str,
    resource_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(warn, "lease", "renew_failed", table, resource_id = %resource_id, error = %error,);
}

pub(crate) fn log_lease_claimed(table: &str, resource_id: &str, context: &str, node_id: &str) {
    crate::observability::log::slog!(info, "lease", "claimed", table, resource_id = %resource_id, context = %context, node_id,);
}

pub(crate) fn log_lease_on_acquired_failed(
    table: &str,
    resource_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(warn, "lease", "on_acquired_failed", table, resource_id = %resource_id, error = %error,);
}

pub(crate) fn log_lease_claim_failed(
    table: &str,
    resource_id: &str,
    error: &impl std::fmt::Display,
) {
    crate::observability::log::slog!(warn, "lease", "claim_failed", table, resource_id = %resource_id, error = %error,);
}
