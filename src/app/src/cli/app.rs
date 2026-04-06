use std::sync::Arc;

use crate::cli::args::CliArgs;
use crate::cli::args::CliCommand;
use crate::cli::create_sink;
use crate::cli::repl::Repl;
use crate::conf::load_config;
use crate::conf::ConfigOverrides;
use crate::error::BendclawError;
use crate::error::Result;
use crate::request::ExecutionLimits;
use crate::request::Request;
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
            (None, None) => self.run_repl().await,
            (Some(prompt), None) => self.run_prompt(prompt.clone()).await,
            (None, Some(CliCommand::Repl)) => self.run_repl().await,
            (None, Some(CliCommand::Server(server_args))) => {
                self.run_server(server_args.port).await
            }
        }
    }

    fn build_limits(&self) -> ExecutionLimits {
        ExecutionLimits {
            max_turns: self.args.max_turns,
            max_total_tokens: self.args.max_tokens,
            max_duration_secs: self.args.max_duration,
        }
    }

    fn build_request(&self, prompt: String) -> Request {
        let mut request = Request::new(prompt).with_limits(self.build_limits());
        if let Some(id) = &self.args.resume {
            request = request.with_session(id.clone());
        }
        if let Some(sp) = &self.args.append_system_prompt {
            request = request.with_system_prompt(sp.clone());
        }
        request
    }

    async fn run_prompt(&self, prompt: String) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), None))?;
        let storage = open_storage(&config.storage)?;
        let sink = create_sink(&self.args.output_format);
        let request = self.build_request(prompt);
        let _ = request.execute(config.active_llm(), sink, storage).await?;
        Ok(())
    }

    async fn run_server(&self, port: Option<u16>) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), port))?;
        server::start(config).await
    }

    async fn run_repl(&self) -> Result<()> {
        let config = load_config(ConfigOverrides::new(self.args.model.clone(), None))?;
        let storage = open_storage(&config.storage)?;
        Repl::new(
            config,
            storage,
            self.build_limits(),
            self.args.append_system_prompt.clone(),
            self.args.resume.clone(),
        )?
        .run()
        .await
    }
}

pub async fn run_cli(args: CliArgs) -> Result<()> {
    Cli::new(args).run().await
}
