use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = evot::cli::CliArgs::parse();

    let quiet_prompt_mode = args.command.is_none() && args.prompt.is_some() && !args.verbose;

    if !quiet_prompt_mode {
        let env_filter = match tracing_subscriber::EnvFilter::try_from_default_env() {
            Ok(filter) => filter,
            Err(_) => {
                if args.verbose {
                    "info,bendengine=warn".into()
                } else {
                    "warn,bendengine=warn".into()
                }
            }
        };

        tracing_subscriber::fmt().with_env_filter(env_filter).init();

        tracing::info!(
            stage = "app",
            status = "started",
            git_sha = env!("EVOT_GIT_SHA"),
            git_branch = env!("EVOT_GIT_BRANCH"),
            build_timestamp = env!("EVOT_BUILD_TIMESTAMP"),
            "build info",
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        evot::cli::run_cli(args)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
}
