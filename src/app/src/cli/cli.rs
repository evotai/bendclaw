use std::path::PathBuf;
use std::sync::Arc;

use super::format::format_tool_input;
use super::format::truncate;
use crate::agent::prompt::SystemPrompt;
use crate::agent::AppAgent;
use crate::agent::ExecutionLimits;
use crate::agent::RunEvent;
use crate::agent::RunEventPayload;
use crate::agent::TurnRequest;
use crate::agent::Variables;
use crate::cli::args::CliArgs;
use crate::cli::args::CliCommand;
use crate::cli::args::OutputFormat;
use crate::cli::repl::Repl;
use crate::conf::Config;
use crate::error::BendclawError;
use crate::error::Result;
use crate::server;

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub async fn run_cli(args: CliArgs) -> Result<()> {
    Cli::new(args).run().await
}

// ---------------------------------------------------------------------------
// Cli
// ---------------------------------------------------------------------------

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

    // -- subcommand dispatch ------------------------------------------------

    async fn run_prompt(&self, prompt: String) -> Result<()> {
        let config = Config::load()?.with_model(self.args.model.clone());
        let cwd = current_dir()?;
        let mut builder = SystemPrompt::new(&cwd)
            .with_system()
            .with_git()
            .with_tools()
            .with_project_context()
            .with_memory();
        if let Some(extra) = self.args.append_system_prompt.as_deref() {
            builder = builder.with_append(extra);
        }
        let system_prompt = builder.build();
        let agent = AppAgent::new(&config, &cwd)?
            .with_system_prompt(system_prompt)
            .with_limits(self.build_limits())
            .with_skills_dirs(self.build_skills_dirs());

        // Load variables from storage.
        let storage = agent.storage();
        let records = storage.load_variables().await.unwrap_or_default();
        let variables = Arc::new(Variables::new(storage, records));
        agent.with_variables(variables.clone());

        let secret_values = variables.secret_values();
        let request = TurnRequest::text(prompt).session_id(self.args.resume.clone());
        let mut stream = agent.submit(request).await?;
        while let Some(event) = stream.next().await {
            print_event(&event, &self.args.output_format, &secret_values);
        }
        Ok(())
    }

    async fn run_server(&self, port: Option<u16>) -> Result<()> {
        let mut config = Config::load()?.with_model(self.args.model.clone());
        if let Some(p) = port {
            config = config.with_port(p);
        }
        server::start(config).await
    }

    async fn run_repl(&self) -> Result<()> {
        let config = Config::load()?.with_model(self.args.model.clone());
        let cwd = current_dir()?;
        let mut builder = SystemPrompt::new(&cwd)
            .with_system()
            .with_git()
            .with_tools()
            .with_project_context()
            .with_memory();
        if let Some(extra) = self.args.append_system_prompt.as_deref() {
            builder = builder.with_append(extra);
        }
        let system_prompt = builder.build();
        let agent = AppAgent::new(&config, &cwd)?
            .with_system_prompt(system_prompt)
            .with_limits(self.build_limits())
            .with_skills_dirs(self.build_skills_dirs());

        // Load variables from storage.
        let storage = agent.storage();
        let records = storage.load_variables().await.unwrap_or_default();
        let variables = Arc::new(Variables::new(storage, records));
        agent.with_variables(variables);

        Repl::new_with_agent(agent, config, self.args.resume.clone())?
            .run()
            .await
    }

    // -- helpers ------------------------------------------------------------

    fn build_limits(&self) -> ExecutionLimits {
        ExecutionLimits {
            max_turns: self.args.max_turns,
            max_total_tokens: self.args.max_tokens,
            max_duration_secs: self.args.max_duration,
        }
    }

    fn build_skills_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Ok(global) = crate::conf::paths::skills_dir() {
            dirs.push(global);
        }
        for extra in &self.args.skills_dirs {
            dirs.push(PathBuf::from(extra));
        }
        dirs
    }
}

fn current_dir() -> Result<String> {
    std::env::current_dir()
        .map_err(|e| BendclawError::Run(format!("failed to get cwd: {e}")))
        .map(|p| p.to_string_lossy().to_string())
}

use super::format::mask_secrets;

fn print_event(event: &RunEvent, format: &OutputFormat, secret_values: &[String]) {
    match format {
        OutputFormat::Text => print_event_text(event, secret_values),
        OutputFormat::StreamJson => print_event_json(event, secret_values),
    }
}

fn print_event_text(event: &RunEvent, secret_values: &[String]) {
    match &event.payload {
        RunEventPayload::AssistantCompleted { content, .. } => {
            for block in content {
                match block {
                    crate::agent::AssistantBlock::Text { .. } => {}
                    crate::agent::AssistantBlock::ToolCall { name, input, .. } => {
                        let detail = format_tool_input(input);
                        eprintln!("[call: {name}] {detail}");
                    }
                    crate::agent::AssistantBlock::Thinking { .. } => {}
                }
            }
        }
        RunEventPayload::ToolFinished {
            tool_name,
            content,
            is_error,
            ..
        } => {
            let masked = mask_secrets(content, secret_values);
            if *is_error {
                eprintln!("[error: {tool_name}] {masked}");
            } else if !masked.is_empty() {
                eprintln!("[done: {tool_name}] {}", truncate(&masked, 120));
            }
        }
        RunEventPayload::AssistantDelta {
            delta: Some(delta), ..
        } => {
            print!("{delta}");
        }
        RunEventPayload::Error { message } => {
            eprintln!("error: {message}");
        }
        RunEventPayload::ToolProgress { text, .. } => {
            let masked = mask_secrets(text, secret_values);
            eprintln!("[{masked}]");
        }
        RunEventPayload::RunFinished { .. } => {
            println!();
        }
        _ => {}
    }
}

fn print_event_json(event: &RunEvent, secret_values: &[String]) {
    if secret_values.is_empty() {
        if let Ok(json) = serde_json::to_string(event) {
            println!("{json}");
        }
        return;
    }
    if let Ok(json) = serde_json::to_string(event) {
        println!("{}", mask_secrets(&json, secret_values));
    }
}
