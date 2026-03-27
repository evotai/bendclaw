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
use tracing::info;
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
        Command::Update => bendclaw::cli::cmd_update().await?,
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
    // Load console-injected env vars (.env file < shell env < CLI)
    let env_file = bendclaw::cli::evotai_dir().join("bendclaw.env");
    if env_file.exists() {
        dotenvy::from_path(&env_file).ok();
    }

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
        let writer = match tracing_fmt::LocalDailyWriter::new(&config.log.dir, "bendclaw.log") {
            Ok(w) => w,
            Err(e) => {
                eprintln!("failed to create log file writer: {e}");
                std::process::exit(1);
            }
        };
        let file_filter = EnvFilter::builder()
            .parse(format!("{},tower_http=warn", &config.log.level))
            .unwrap_or_else(|_| EnvFilter::new("info,tower_http=warn"));
        let base = tracing_subscriber::fmt::layer()
            .with_ansi(false)
            .with_target(true)
            .with_writer(writer);
        let layer = if config.log.format == "json" {
            base.json()
                .flatten_event(true)
                .with_current_span(false)
                .with_span_list(false)
                .boxed()
        } else {
            base.event_format(tracing_fmt::TargetFirstFormatter::new())
                .boxed()
        };
        Some(layer.with_filter(file_filter))
    } else {
        None
    };

    let terminal_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .event_format(tracing_fmt::TargetFirstFormatter::new());

    // Sentry error reporting — enabled by default, opt out via config or env.
    let _sentry_guard = if config.telemetry.enabled {
        let guard = sentry::init((
            "https://2d81d3539114623bd11edfa8ae3f7cb5@o1250071.ingest.us.sentry.io/4511053422723072",
            sentry::ClientOptions {
                release: sentry::release_name!(),
                ..Default::default()
            },
        ));
        sentry::configure_scope(|scope| {
            scope.set_user(Some(sentry::User {
                id: Some(config.node_id.clone()),
                ..Default::default()
            }));
            scope.set_tag("os", std::env::consts::OS);
            scope.set_tag("arch", std::env::consts::ARCH);
            scope.set_tag("git_sha", bendclaw::version::BENDCLAW_GIT_SHA);
        });
        Some(guard)
    } else {
        None
    };

    let sentry_layer = if config.telemetry.enabled {
        Some(
            sentry_tracing::layer().event_filter(|md| match *md.level() {
                tracing::Level::ERROR => sentry_tracing::EventFilter::Event,
                _ => sentry_tracing::EventFilter::Ignore,
            }),
        )
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(terminal_layer.with_filter(filter))
        .with(file_layer)
        .with(sentry_layer)
        .with(ErrorLayer::default())
        .init();

    config.validate()?;
    print_banner(&config);
    config.log_non_defaults();

    let llm = Arc::new(LLMRouter::from_config(&config.llm)?);

    let runtime = Runtime::new(
        &config.storage.databend_api_base_url,
        &config.storage.databend_api_token,
        &config.storage.databend_warehouse,
        &config.storage.db_prefix,
        &config.node_id,
        llm,
    )
    .with_hub_config(config.hub.clone())
    .with_workspace(config.workspace.clone())
    .with_cluster_config(config.cluster.clone(), &config.auth.api_key)
    .with_directive_config(config.directive.clone())
    .build()
    .await?;

    let shutdown_token = tokio_util::sync::CancellationToken::new();

    let state = AppState {
        runtime: runtime.clone(),
        auth_key: config.auth.api_key.clone(),
        shutdown_token: shutdown_token.clone(),
    };

    let api_router = bendclaw::service::api_router(state, &config.log.level, &config.auth);

    let api_bind = &config.server.bind_addr;
    let api_listener = tokio::net::TcpListener::bind(api_bind).await?;
    let api_local_addr = api_listener.local_addr()?;

    let admin_listener = if let Some(ref admin) = config.admin {
        let admin_state = bendclaw::service::AdminState {
            runtime: runtime.clone(),
            shutdown_token: shutdown_token.clone(),
        };
        let listener = tokio::net::TcpListener::bind(&admin.bind_addr).await?;
        let admin_local_addr = listener.local_addr()?;
        info!(
            stage = "server",
            status = "ready",
            api_addr = %api_local_addr,
            admin_addr = %admin_local_addr,
            "server ready"
        );
        Some((listener, bendclaw::service::admin_router(admin_state)))
    } else {
        info!(
            stage = "server",
            status = "ready",
            api_addr = %api_local_addr,
            "server ready"
        );
        None
    };

    let api_shutdown = shutdown_token.clone();
    let api_server = axum::serve(api_listener, api_router)
        .with_graceful_shutdown(async move { api_shutdown.cancelled().await });
    let admin_server = admin_listener.map(|(listener, router)| {
        let admin_shutdown = shutdown_token.clone();
        axum::serve(listener, router)
            .with_graceful_shutdown(async move { admin_shutdown.cancelled().await })
    });

    bendclaw::service::server::supervise_servers(shutdown_token, api_server, admin_server, async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;

    // Graceful shutdown with 60s hard deadline; second Ctrl+C forces exit.
    let force_exit = tokio::spawn(async {
        let _ = tokio::signal::ctrl_c().await;
        tracing::warn!(stage = "server", status = "force_exit", "server force_exit");
        std::process::exit(1);
    });

    let shutdown_deadline = std::time::Duration::from_secs(60);
    if tokio::time::timeout(shutdown_deadline, async {
        if let Err(e) = runtime.shutdown().await {
            tracing::warn!(stage = "server", status = "shutdown_error", error = %e, "server shutdown_error");
        }
    })
    .await
    .is_err()
    {
        tracing::error!(stage = "server", status = "shutdown_timeout", "server shutdown_timeout");
        std::process::exit(1);
    }

    force_exit.abort();

    Ok(())
}

fn print_banner(config: &BendClawConfig) {
    use std::fmt::Write;

    use bendclaw::version;

    let ver = version::commit_version();
    let line = "─".repeat(56);

    let mut buf = format!(
        "\n\
         {line}\n  \
         BendClaw {ver}\n\
         {line}\n"
    );

    let _ = writeln!(buf, "  node_id:      {}", config.node_id);
    let _ = writeln!(buf, "  db_prefix:    {}", config.storage.db_prefix);
    let _ = writeln!(
        buf,
        "  db_api:       {}",
        config.storage.databend_api_base_url
    );
    let _ = writeln!(buf, "  warehouse:    {}", config.storage.databend_warehouse);
    let _ = writeln!(buf, "  api:          {}", config.server.bind_addr);

    for (index, provider) in config.llm.providers.iter().enumerate() {
        let _ = writeln!(
            buf,
            "  llm[{index}]:      {} | {} | {} | {}",
            provider.name, provider.provider, provider.model, provider.base_url
        );
    }

    if let Some(ref admin) = config.admin {
        let _ = writeln!(buf, "  admin:        {}", admin.bind_addr);
    }

    if let Some(ref cluster) = config.cluster {
        let _ = writeln!(buf, "  cluster_id:   {}", cluster.cluster_id);
        if !cluster.advertise_url.is_empty() {
            let _ = writeln!(buf, "  advertise:    {}", cluster.advertise_url);
        }
    }

    if config.hub.is_some() {
        let _ = writeln!(buf, "  hub:          enabled");
    }

    if config.directive.is_some() {
        let _ = writeln!(buf, "  directive:    enabled");
    }

    let _ = writeln!(buf, "  log_level:    {}", config.log.level);

    if !config.log.dir.is_empty() {
        let _ = writeln!(buf, "  log_dir:      {}", config.log.dir);
    }

    let _ = writeln!(buf, "{line}");

    eprint!("{buf}");
}
