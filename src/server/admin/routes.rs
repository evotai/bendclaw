use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;

use super::state::AdminState;
use crate::observability::log::slog;
use crate::runtime::SuspendStatus;

pub async fn can_suspend(State(state): State<AdminState>) -> Json<SuspendStatus> {
    Json(state.runtime.suspend_status())
}

#[derive(Serialize)]
pub struct UpgradeResponse {
    pub status: &'static str,
    pub from_version: String,
    pub to_version: String,
}

pub async fn upgrade(
    State(state): State<AdminState>,
) -> Result<Json<UpgradeResponse>, (StatusCode, String)> {
    // Reject if there are active sessions/tasks.
    let suspend = state.runtime.suspend_status();
    if !suspend.can_suspend {
        return Err((
            StatusCode::CONFLICT,
            format!(
                "cannot upgrade: {} active sessions, {} active tasks, {} held leases",
                suspend.active_sessions, suspend.active_tasks, suspend.active_leases
            ),
        ));
    }

    let from_version = crate::cli::update::current_release_tag();

    // Check for latest release.
    let release = crate::cli::update::fetch_latest_release()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                format!("failed to check release: {e}"),
            )
        })?;

    let to_version = release.tag_name.clone();

    if crate::cli::update::tags_match(&from_version, &to_version) {
        return Ok(Json(UpgradeResponse {
            status: "already_latest",
            from_version: from_version.trim_start_matches('v').to_string(),
            to_version: to_version.trim_start_matches('v').to_string(),
        }));
    }

    // Download and install.
    let target = crate::cli::update::supported_target().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unsupported platform: {e}"),
        )
    })?;

    let asset = crate::cli::update::select_asset(&release, target).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            format!("no release asset for target {target}"),
        )
    })?;

    let archive = crate::cli::update::download_asset(asset)
        .await
        .map_err(|e| (StatusCode::BAD_GATEWAY, format!("download failed: {e}")))?;

    let binary = crate::cli::update::extract_binary(&archive).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("extract failed: {e}"),
        )
    })?;

    let current_exe = std::env::current_exe().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cannot resolve exe: {e}"),
        )
    })?;

    crate::cli::update::install_binary(&current_exe, &binary).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("install failed: {e}"),
        )
    })?;

    slog!(
        info,
        "server",
        "upgrade_completed",
        from = from_version.trim_start_matches('v'),
        to = to_version.trim_start_matches('v'),
    );

    // Schedule graceful shutdown after response is sent.
    let shutdown_token = state.shutdown_token.clone();
    crate::types::spawn_fire_and_forget("graceful_shutdown_delay", async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        shutdown_token.cancel();
    });

    Ok(Json(UpgradeResponse {
        status: "upgraded",
        from_version: from_version.trim_start_matches('v').to_string(),
        to_version: to_version.trim_start_matches('v').to_string(),
    }))
}
