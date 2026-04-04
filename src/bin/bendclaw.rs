use clap::Parser;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    tracing::info!(
        git_sha = env!("BENDCLAW_GIT_SHA"),
        git_branch = env!("BENDCLAW_GIT_BRANCH"),
        build_timestamp = env!("BENDCLAW_BUILD_TIMESTAMP"),
        "build info"
    );

    let args = bendclaw::cli::CliArgs::parse();

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        bendclaw::cli::run_cli(args)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
}
