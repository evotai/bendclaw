use bend_base::logx;
use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = bendclaw::cli::CliArgs::parse();

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

        logx!(
            info,
            "app",
            "started",
            msg = "build info",
            git_sha = env!("BENDCLAW_GIT_SHA"),
            git_branch = env!("BENDCLAW_GIT_BRANCH"),
            build_timestamp = env!("BENDCLAW_BUILD_TIMESTAMP"),
        );
    }

    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        bendclaw::cli::run_cli(args)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    })
}
