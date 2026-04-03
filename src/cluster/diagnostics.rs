pub(crate) fn log_cluster_peers_refreshed(peer_count: usize, elapsed_ms: u64) {
    crate::observability::log::slog!(info, "cluster", "peers_refreshed", peer_count, elapsed_ms,);
}

pub(crate) fn log_cluster_peers_unchanged(peer_count: usize, elapsed_ms: u64) {
    crate::observability::log::slog!(debug, "cluster", "peers_unchanged", peer_count, elapsed_ms,);
}

pub(crate) fn log_cluster_discovery_completed(peer_count: usize) {
    crate::observability::log::slog!(info, "cluster", "discovery_completed", peer_count,);
}

pub(crate) fn log_cluster_discovery_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "cluster", "discovery_failed", error = %error,);
}

pub(crate) fn log_cluster_heartbeat_started(heartbeat_interval_ms: u64) {
    crate::observability::log::slog!(info, "cluster", "heartbeat_started", heartbeat_interval_ms,);
}

pub(crate) fn log_cluster_heartbeat_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "cluster", "heartbeat_failed", error = %error,);
}

pub(crate) fn log_cluster_refresh_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "cluster", "refresh_failed", error = %error,);
}

pub(crate) fn log_cluster_deregistration_failed(error: &impl std::fmt::Display) {
    crate::observability::log::slog!(warn, "cluster", "deregistration_failed", error = %error,);
}
