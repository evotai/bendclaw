use anyhow::Context as _;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;

use crate::observability::log::slog;
use crate::service::state::AppState;

#[derive(Deserialize, Default)]
pub struct UpgradeRequest {
    pub target_version: Option<String>,
    pub download_base_url: Option<String>,
}

#[derive(Serialize)]
pub struct UpgradeResponse {
    pub status: &'static str,
    pub from_version: String,
    pub to_version: String,
}

pub async fn upgrade(
    State(state): State<AppState>,
    Json(body): Json<UpgradeRequest>,
) -> Result<Json<UpgradeResponse>, (StatusCode, String)> {
    let from_version = crate::cli::update::current_release_tag();
    let to_version = body.target_version.clone().unwrap_or_default();

    let base_url = body.download_base_url.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            "missing download_base_url".to_string(),
        )
    })?;

    let target = crate::cli::update::supported_target().map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("unsupported platform: {e}"),
        )
    })?;

    let download_url = format!("{base_url}/{target}");

    slog!(info, "server", "upgrade_downloading",
        download_url = %download_url,
        target = %target,
    );

    let archive = download_binary(&download_url).await.map_err(|e| {
        slog!(error, "server", "upgrade_download_failed",
            download_url = %download_url,
            error = %e,
        );
        (
            StatusCode::BAD_GATEWAY,
            format!("download failed from {download_url}: {e}"),
        )
    })?;

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

async fn download_binary(url: &str) -> anyhow::Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .build()
        .context("failed to build HTTP client")?;

    let resp = client
        .get(url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .with_context(|| format!("failed to GET {url}"))?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("HTTP {status}: {body}");
    }

    Ok(resp
        .bytes()
        .await
        .context("failed to read response bytes")?
        .to_vec())
}
