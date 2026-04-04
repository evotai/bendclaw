use std::path::PathBuf;

use clap::Parser;

#[derive(Parser)]
#[command(name = "bendclaw", about = "Self-evolving AI agent runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Start the bendclaw server
    Run {
        /// Path to config file
        #[arg(long, short)]
        config: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let cli = Cli::parse();

    match cli.command {
        Command::Run { config } => {
            let cfg = bendclaw::config::Config::load(&config)?;

            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(run(cfg))
        }
    }
}

async fn run(config: bendclaw::config::Config) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tracing::info!(
        node_id = %config.node_id,
        bind_addr = %config.server.bind_addr,
        "starting bendclaw"
    );

    tracing::info!(
        git_sha = env!("BENDCLAW_GIT_SHA"),
        git_branch = env!("BENDCLAW_GIT_BRANCH"),
        build_timestamp = env!("BENDCLAW_BUILD_TIMESTAMP"),
        "build info"
    );

    // TODO: initialize agent manager, HTTP server, etc.

    Ok(())
}
