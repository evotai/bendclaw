use crate::cli::args::CliArgs;
use crate::cli::args::CliCommand;
use crate::cli::args::OutputFormat;
use crate::cli::create_sink;
use crate::conf::load_config;
use crate::conf::ConfigOverrides;
use crate::error::BendclawError;
use crate::error::Result;
use crate::run;
use crate::run::RunRequest;
use crate::server;
use crate::storage::open_storage;

pub async fn run_cli(args: CliArgs) -> Result<()> {
    match (args.prompt, args.command) {
        (Some(prompt), None) => {
            run_prompt(
                prompt,
                args.resume,
                args.output_format,
                args.model,
                args.max_turns,
                args.append_system_prompt,
            )
            .await
        }
        (None, Some(CliCommand::Server(server_args))) => {
            run_server(args.model, server_args.port).await
        }
        (Some(_), Some(_)) => Err(BendclawError::Cli(
            "prompt mode and subcommand cannot be used together".into(),
        )),
        (None, None) => Err(BendclawError::Cli(
            "missing mode: use -p/--prompt or the server subcommand".into(),
        )),
    }
}

async fn run_prompt(
    prompt: String,
    resume: Option<String>,
    output_format: OutputFormat,
    model: Option<String>,
    max_turns: Option<u32>,
    append_system_prompt: Option<String>,
) -> Result<()> {
    let config = load_config(ConfigOverrides::new(model, None))?;
    let storage = open_storage(&config.storage)?;
    let sink = create_sink(&output_format);
    let mut request = RunRequest::new(prompt);
    request.session_id = resume;
    request.max_turns = max_turns;
    request.append_system_prompt = append_system_prompt;

    let _ = run::run(
        request,
        config.active_llm(),
        sink.as_ref(),
        storage.as_ref(),
    )
    .await?;

    Ok(())
}

async fn run_server(model: Option<String>, port: Option<u16>) -> Result<()> {
    let config = load_config(ConfigOverrides::new(model, port))?;
    server::start(config).await
}
