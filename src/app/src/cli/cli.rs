use std::sync::Arc;

use crate::cli::args::CliArgs;
use crate::cli::args::CliCommand;
use crate::cli::create_sink;
use crate::conf::load_config;
use crate::conf::ConfigOverrides;
use crate::error::BendclawError;
use crate::error::Result;
use crate::request::Request;
use crate::request::RequestExecutor;
use crate::server;
use crate::storage::open_storage;

pub struct Cli {
    args: CliArgs,
}

impl Cli {
    pub fn new(args: CliArgs) -> Arc<Self> {
        Arc::new(Self { args })
    }

    pub async fn run(&self) -> Result<()> {
        match (&self.args.prompt, &self.args.command) {
            (Some(_), Some(_)) => Err(BendclawError::Cli(
                "prompt mode and subcommand cannot be used together".into(),
            )),
            (None, None) => Err(BendclawError::Cli(
                "missing mode: use -p/--prompt or the server subcommand".into(),
            )),
            (Some(prompt), None) => self.run_prompt(prompt.clone()).await,
            (None, Some(CliCommand::Server(server_args))) => {
                self.run_server(server_args.port).await
            }
        }
    }

    async fn run_prompt(&self, prompt: String) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), None))?;
        let storage = open_storage(&config.storage)?;
        let sink = create_sink(&self.args.output_format);
        let mut request = Request::new(prompt);
        request.session_id = self.args.resume.clone();
        request.max_turns = self.args.max_turns;
        request.append_system_prompt = self.args.append_system_prompt.clone();

        let _ = RequestExecutor::open(request, config.active_llm(), sink, storage)
            .execute()
            .await?;
        Ok(())
    }

    async fn run_server(&self, port: Option<u16>) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), port))?;
        server::start(config).await
    }
}

pub async fn run_cli(args: CliArgs) -> Result<()> {
    Cli::new(args).run().await
}
