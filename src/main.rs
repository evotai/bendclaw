use std::sync::Arc;

use bendclaw::cli::cmd_restart;
use bendclaw::cli::cmd_start;
use bendclaw::cli::cmd_status;
use bendclaw::cli::cmd_stop;
use bendclaw::cli::Cli;
use bendclaw::cli::Command;
use bendclaw::config::BendClawConfig;
use bendclaw::kernel::Runtime;
use bendclaw::llm::router::LLMRouter;
use bendclaw::service::state::AppState;
use clap::Parser;
use tracing_error::ErrorLayer;
use tracing_subscriber::layer::SubscriberExt as _;
use tracing_subscriber::util::SubscriberInitExt as _;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::Layer as _;

mod tracing_fmt;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command.unwrap_or(Command::Run) {
        Command::Start => cmd_start(),
        Command::Stop => cmd_stop(),
        Command::Restart => cmd_restart(),
        Command::Status => cmd_status(),
        Command::Run => cmd_run(cli.config, cli.overrides).await?,
    }

    Ok(())
}

fn resolve_config_path(explicit: Option<String>) -> String {
    if let Some(path) = explicit {
        return path;
    }
    let default = bendclaw::cli::default_config_path();
    if default.exists() {
        return default.to_string_lossy().into_owned();
    }
    // No file found — will use built-in defaults
    String::new()
}

async fn cmd_run(
    config_path: Option<String>,
    overrides: bendclaw::cli::CliOverrides,
) -> anyhow::Result<()> {
    let path = resolve_config_path(config_path);
    let mut config = if path.is_empty() {
        let mut cfg = BendClawConfig::default();
        cfg.apply_env();
        cfg
    } else {
        BendClawConfig::load(&path)?
    };
    config.apply_cli(&overrides);

    // Logging configured from merged config (no DB dependency).
    let filter = EnvFilter::builder()
        .parse(format!("{},tower_http=warn", &config.log.level))
        .unwrap_or_else(|_| EnvFilter::new("info,tower_http=warn"));

    let file_layer = if !config.log.dir.is_empty() {
        let _ = std::fs::create_dir_all(&config.log.dir);
        let file_appender = tracing_appender::rolling::daily(&config.log.dir, "bendclaw.log");
        Some(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_target(true)
                .with_writer(file_appender),
        )
    } else {
        None
    };

    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .event_format(tracing_fmt::TargetFirstFormatter);

    tracing_subscriber::registry()
        .with(terminal_layer.with_filter(filter))
        .with(file_layer.map(|l| {
            let file_filter = EnvFilter::builder()
                .parse(format!("{},tower_http=warn", &config.log.level))
                .unwrap_or_else(|_| EnvFilter::new("info,tower_http=warn"));
            l.with_filter(file_filter)
        }))
        .with(ErrorLayer::default())
        .init();

    bendclaw::version::log_version();

    config.validate()?;
    config.log_non_defaults();

    let llm = Arc::new(LLMRouter::from_config(&config.llm)?);

    let runtime = Runtime::new(
        &config.storage.databend_api_base_url,
        &config.storage.databend_api_token,
        &config.storage.databend_warehouse,
        &config.storage.db_prefix,
        &config.instance_id,
        llm,
    )
    .with_hub_config(config.hub.clone())
    .with_workspace(config.workspace.clone())
    .with_cluster_config(config.cluster.clone(), &config.auth.api_key)
    .build()
    .await?;

    let state = AppState {
        runtime: runtime.clone(),
        auth_key: config.auth.api_key.clone(),
    };

    // Resume channel receivers for existing accounts.
    bendclaw::service::v1::channels::ChannelAccountService::new(&state)
        .resume_all_receivers()
        .await;

    let api_router = bendclaw::service::api_router(state, &config.log.level, &config.auth);

    let api_bind = &config.server.bind_addr;
    let api_listener = tokio::net::TcpListener::bind(api_bind).await?;
    tracing::info!(bind = %api_bind, "server listening");

    let shutdown_notify = Arc::new(tokio::sync::Notify::new());
    let n1 = shutdown_notify.clone();

    tokio::select! {
        r = axum::serve(api_listener, api_router)
            .with_graceful_shutdown(async move { n1.notified().await }) => r?,
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("shutdown signal received");
            shutdown_notify.notify_waiters();
        }
    }

    // Graceful shutdown: deregister from cluster, cancel heartbeat, close sessions
    if let Err(e) = runtime.shutdown().await {
        tracing::warn!(error = %e, "runtime shutdown error");
    }

    Ok(())
}
