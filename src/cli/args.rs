use std::sync::LazyLock;

use clap::Args;
use clap::Parser;
use clap::Subcommand;

static SHORT_VERSION: LazyLock<String> = LazyLock::new(|| {
    let tag = crate::version::BENDCLAW_GIT_TAG;
    if !tag.is_empty() && tag != "unknown" {
        tag.trim_start_matches('v').to_string()
    } else {
        crate::version::BENDCLAW_VERSION.to_string()
    }
});

static LONG_VERSION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}\ncommit: {}\nbranch: {}\nbuild:  {}\nrustc:  {}\nprofile: {}",
        crate::version::BENDCLAW_GIT_TAG,
        crate::version::BENDCLAW_GIT_SHA,
        crate::version::BENDCLAW_GIT_BRANCH,
        crate::version::BENDCLAW_BUILD_TIMESTAMP,
        crate::version::BENDCLAW_RUSTC_VERSION,
        crate::version::BENDCLAW_BUILD_PROFILE,
    )
});

#[derive(Parser)]
#[command(
    name = "bendclaw",
    about = "🦞 BendClaw — enterprise-grade OpenClaw service",
    version = SHORT_VERSION.as_str(),
    long_version = LONG_VERSION.as_str(),
)]
pub struct Cli {
    /// Path to the TOML configuration file.
    #[arg(short, long, value_name = "PATH")]
    pub config: Option<String>,

    #[command(subcommand)]
    pub command: Option<Command>,

    /// CLI argument overrides — highest priority, beats file and env vars.
    #[clap(flatten)]
    pub overrides: CliOverrides,
}

/// CLI-level config overrides. Every field is optional; only set values are applied.
/// Priority: CLI args > env vars > config file > built-in defaults.
#[derive(Debug, Default, Args)]
pub struct CliOverrides {
    /// Databend Cloud API base URL (overrides BENDCLAW_STORAGE_DATABEND_API_BASE_URL and config file).
    #[clap(long, value_name = "URL")]
    pub storage_api_base_url: Option<String>,

    /// Databend Cloud API token (overrides BENDCLAW_STORAGE_DATABEND_API_TOKEN and config file).
    #[clap(long, value_name = "TOKEN")]
    pub storage_api_token: Option<String>,

    /// Databend Cloud warehouse name (overrides BENDCLAW_STORAGE_DATABEND_WAREHOUSE and config file).
    #[clap(long, value_name = "WAREHOUSE")]
    pub storage_warehouse: Option<String>,

    /// Server bind address, e.g. 0.0.0.0:8787.
    #[clap(long, value_name = "ADDR")]
    pub bind_addr: Option<String>,

    /// Auth API key for Bearer token authentication (overrides config file).
    #[clap(long, value_name = "KEY")]
    pub auth_key: Option<String>,

    /// Log level: error / warn / info / debug / trace.
    #[clap(long, value_name = "LEVEL")]
    pub log_level: Option<String>,

    /// Log format: text / json.
    #[clap(long, value_name = "FORMAT")]
    pub log_format: Option<String>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start server in background
    Start,
    /// Stop the server
    Stop,
    /// Kill old process, start new one
    Restart,
    /// Show server status
    Status,
    /// Download and install the latest stable release binary
    Update,
    /// Run in foreground (default)
    Run,
    /// Run a local agent session
    Agent(super::agent_cmd::AgentArgs),
}
