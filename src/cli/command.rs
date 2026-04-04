use crate::cli::args::CliArgs;
use crate::cli::output;
use crate::env::load_runtime_env;
use crate::env::paths;
use crate::error::Result;
use crate::run;
use crate::run::RunRequest;
use crate::store::create_stores;
use crate::store::StoreBackend;

pub async fn run_cli(args: CliArgs) -> Result<()> {
    let runtime_env = load_runtime_env(args.model.as_deref())?;

    let mut request = RunRequest::new(args.prompt);
    request.session_id = args.resume;

    let sink = output::create_sink(&args.output_format);
    let stores = create_stores(StoreBackend::Fs {
        session_dir: paths::sessions_dir()?,
        run_dir: paths::runs_dir()?,
    })?;

    run::run(
        request,
        runtime_env.llm,
        sink.as_ref(),
        stores.session.as_ref(),
        stores.run.as_ref(),
    )
    .await
}
