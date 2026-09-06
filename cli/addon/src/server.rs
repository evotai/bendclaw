use napi::bindgen_prelude::*;
use napi_derive::napi;

use crate::tracing::init_tracing;

/// Load and validate config with optional model and port overrides.
fn load_config(
    port: Option<u16>,
    model: Option<String>,
    env_file: Option<String>,
) -> Result<evot::conf::Config> {
    let mut config = evot::conf::Config::load_with_env_file(env_file.as_deref())
        .map_err(|e| Error::from_reason(format!("config load failed: {e}")))?
        .with_model(model)
        .map_err(|e| Error::from_reason(format!("config model: {e}")))?;
    if let Some(p) = port {
        config = config.with_port(p);
    }
    Ok(config)
}

/// Version string for the native addon.
#[napi]
pub fn version() -> String {
    env!("EVOT_VERSION").to_string()
}

#[napi]
pub async fn start_server(
    port: Option<u16>,
    model: Option<String>,
    env_file: Option<String>,
) -> Result<()> {
    init_tracing();
    let config = load_config(port, model, env_file)?;
    evot::gateway::service::start(config)
        .await
        .map_err(|e| Error::from_reason(format!("server error: {e}")))
}

#[napi]
pub async fn start_server_background(
    port: Option<u16>,
    model: Option<String>,
    env_file: Option<String>,
) -> Result<Option<String>> {
    init_tracing();
    let config = load_config(port, model, env_file)?;
    let actual_port = config.server.port;
    let host = config.server.host.clone();
    let addr = format!("{host}:{actual_port}");

    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(_) => return Ok(None),
    };

    let agent = evot::bootstrap::build_agent(&config)
        .await
        .map_err(|e| Error::from_reason(format!("agent init: {e}")))?;

    let cancel = tokio_util::sync::CancellationToken::new();
    let channel_handles =
        evot::gateway::registry::spawn_all(&config.channels, agent.clone(), cancel);

    let mut channels = Vec::new();
    if config.channels.feishu.is_some() {
        channels.push("feishu");
    }

    let server = evot::gateway::channels::http::Server::new(agent, config.clone());
    tokio::spawn(async move {
        let _ = axum::serve(listener, server.router()).await;
    });

    let info = serde_json::json!({
        "port": actual_port,
        "address": format!("http://{addr}"),
        "channels": channels,
        "channelCount": channel_handles.len(),
    });
    serde_json::to_string(&info)
        .map(Some)
        .map_err(|e| Error::from_reason(format!("serialize: {e}")))
}
