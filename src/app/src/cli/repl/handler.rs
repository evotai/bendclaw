use std::fs;
use std::io::Write;
use std::sync::Arc;
use std::sync::RwLock;

use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;

use super::commands::command_help;
use super::commands::command_short_description;
use super::commands::KNOWN_COMMANDS;
use super::completion::CompletionState;
use super::completion::CompletionStateRef;
use super::completion::ReplHelper;
use super::render::print_transcript_messages;
use super::render::truncate;
use super::render::BOLD;
use super::render::DIM;
use super::render::GREEN;
use super::render::RED;
use super::render::RESET;
use super::render::YELLOW;
use super::selector::available_models;
use super::selector::provider_marker_for_model;
use super::selector::run_selector;
use super::selector::wait_for_run_control;
use super::selector::PromptExit;
use super::selector::RunControl;
use super::selector::SelectorOption;
use super::sink::ReplSink;
use crate::agent::AppAgent;
use crate::agent::ExecutionLimits;
use crate::agent::TurnRequest;
use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ProviderKind;
use crate::error::BendclawError;
use crate::error::Result;
use crate::protocol::SessionMeta;

// ---------------------------------------------------------------------------
// Repl
// ---------------------------------------------------------------------------

pub struct Repl {
    agent: Arc<AppAgent>,
    config: Config,
    session_id: Option<String>,
    cwd: String,
    completion_state: CompletionStateRef,
}

impl Repl {
    pub fn new(
        config: Config,
        limits: ExecutionLimits,
        append_system_prompt: Option<String>,
        session_id: Option<String>,
    ) -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|e| BendclawError::Cli(format!("failed to get cwd: {e}")))?
            .to_string_lossy()
            .to_string();

        let system_prompt = Self::resolve_system_prompt(&cwd, append_system_prompt.as_deref());
        let agent = Arc::new(
            AppAgent::new(&config, &cwd)?
                .with_system_prompt(system_prompt)
                .with_limits(limits),
        );

        Ok(Self {
            agent,
            config,
            session_id,
            cwd,
            completion_state: Arc::new(RwLock::new(CompletionState::default())),
        })
    }

    fn resolve_system_prompt(cwd: &str, append: Option<&str>) -> String {
        let mut prompt = format!("You are a helpful assistant. Working directory: {cwd}");
        if let Some(ctx) = crate::cli::context::load_project_context(cwd) {
            prompt.push_str("\n\n# Project Instructions\n\n");
            prompt.push_str(&ctx);
        }
        if let Some(extra) = append {
            prompt.push('\n');
            prompt.push_str(extra);
        }
        prompt
    }

    pub async fn run(&mut self) -> Result<()> {
        self.refresh_completion_state().await?;
        self.print_banner()?;

        if let Some(session_id) = self.session_id.clone() {
            self.resume_session(&session_id, true).await?;
        } else {
            self.print_resume_hint().await?;
        }

        let config = rustyline::config::Builder::new()
            .completion_type(rustyline::config::CompletionType::List)
            .completion_prompt_limit(50)
            .build();
        let mut rl = Editor::with_config(config)
            .map_err(|e| BendclawError::Cli(format!("failed to initialize readline: {e}")))?;
        rl.set_helper(Some(ReplHelper::new(self.completion_state.clone())));
        self.load_history(&mut rl);

        loop {
            let prompt = self.prompt();
            let line = match rl.readline(&prompt) {
                Ok(line) => line,
                Err(ReadlineError::Interrupted) => {
                    println!();
                    break;
                }
                Err(ReadlineError::Eof) => break,
                Err(error) => {
                    return Err(BendclawError::Cli(format!("failed to read input: {error}")));
                }
            };

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let _ = rl.add_history_entry(&line);
            let input = if needs_continuation(trimmed) {
                collect_multiline_rl(trimmed, &mut rl)
            } else {
                line
            };
            let input = input.trim();

            if input.starts_with('/') {
                if self.handle_command(input).await? {
                    break;
                }
            } else if self.run_prompt(input).await? {
                break;
            }

            self.refresh_completion_state().await?;
        }

        self.save_history(&mut rl);
        self.print_resume_hint_on_exit();
        Ok(())
    }

    async fn handle_command(&mut self, input: &str) -> Result<bool> {
        match input {
            "/quit" | "/exit" => return Ok(true),
            "/help" => self.print_help_summary(),
            s if s.starts_with("/help ") => {
                self.print_help_for(s.trim_start_matches("/help ").trim())
            }
            "/status" => self.print_status().await?,
            "/config" => self.print_config(),
            "/version" => self.print_version(),
            "/history" => self.print_history().await?,
            "/sessions" => self.choose_session(false).await?,
            "/sessions all" => self.choose_session(true).await?,
            "/resume" => self.choose_session(false).await?,
            s if s.starts_with("/resume ") => {
                let session_id = self
                    .resolve_session_id(s.trim_start_matches("/resume ").trim())
                    .await?;
                self.resume_session(&session_id, true).await?;
            }
            "/new" => self.start_new_session(false).await?,
            "/clear" => self.start_new_session(true).await?,
            "/clear!" => self.force_new_session(),
            "/model" => self.choose_model().await?,
            s if s.starts_with("/model ") => {
                self.set_model(s.trim_start_matches("/model ").trim())?
            }
            "/provider" => self.print_provider(),
            s if s.starts_with("/provider ") => {
                self.set_provider(s.trim_start_matches("/provider ").trim())?
            }
            _ => {
                eprintln!("{RED}  unknown command: {input}{RESET}");
                eprintln!("{DIM}  type /help for available commands{RESET}\n");
            }
        }

        Ok(false)
    }

    async fn run_prompt(&mut self, input: &str) -> Result<bool> {
        let request = TurnRequest::text(input).session_id(self.session_id.clone());
        let agent = self.agent.clone();
        let spinner_state = Arc::new(std::sync::Mutex::new(super::spinner::SpinnerState::new()));
        let sink = Arc::new(ReplSink::new(spinner_state.clone()));

        let mut run_task = tokio::spawn(async move {
            let mut stream = agent.run(request).await?;
            let session_id = stream.session_id.clone();
            while let Some(event) = stream.next().await {
                sink.render(&event);
            }
            Ok::<_, BendclawError>(session_id)
        });
        let control = wait_for_run_control(&mut run_task, &spinner_state)?;
        let outcome = match control {
            Some(action) => {
                self.agent.abort();
                run_task.abort();
                let _ = run_task.await;
                PromptExit::Cancelled(action == RunControl::Exit)
            }
            None => {
                let session_id = run_task
                    .await
                    .map_err(|e| BendclawError::Cli(format!("request task failed: {e}")))??;
                PromptExit::Finished(session_id, false)
            }
        };

        // Always update session_id
        match &outcome {
            PromptExit::Finished(sid, _) => self.session_id = Some(sid.clone()),
            PromptExit::Cancelled(_) => {
                // session_id may have been set before abort; keep existing
            }
        }

        match outcome {
            PromptExit::Finished(_result, exit_requested) => Ok(exit_requested),
            PromptExit::Cancelled(exit_requested) => {
                println!("{DIM}[stopped]{RESET}");
                Ok(exit_requested)
            }
        }
    }

    async fn resume_session(&mut self, session_id: &str, print_transcript: bool) -> Result<()> {
        let session =
            self.agent.load_session(session_id).await?.ok_or_else(|| {
                BendclawError::Session(format!("session not found: {session_id}"))
            })?;
        let meta = session.meta().await;
        let messages = session.transcript().await;

        self.session_id = Some(meta.session_id.clone());

        // Restore the model from the session without clobbering provider defaults.
        // If the session model matches a known provider config, switch to that provider.
        // Otherwise, set it on the current provider as a custom override.
        let session_model = &meta.model;
        if self.config.anthropic.model == *session_model {
            self.config.llm.provider = ProviderKind::Anthropic;
        } else if self.config.openai.model == *session_model {
            self.config.llm.provider = ProviderKind::OpenAi;
        } else {
            // Custom model — apply to current provider without overwriting the default
            let provider = self.config.llm.provider.clone();
            self.config.provider_config_mut(&provider).model = session_model.clone();
        }
        self.agent.set_llm(self.config.active_llm());

        println!(
            "{DIM}  resumed {}  ·  {}  ·  {} turns{RESET}",
            short_id(&meta.session_id),
            meta.model,
            meta.turns
        );
        if print_transcript && !messages.is_empty() {
            println!();
            print_transcript_messages(&messages);
        }
        Ok(())
    }

    async fn start_new_session(&mut self, confirm: bool) -> Result<()> {
        if confirm && !self.confirm_clear().await? {
            println!("{DIM}  (clear cancelled){RESET}\n");
            return Ok(());
        }
        self.force_new_session();
        Ok(())
    }

    fn force_new_session(&mut self) {
        self.session_id = None;
        println!("{DIM}  (started new session){RESET}\n");
    }

    async fn confirm_clear(&self) -> Result<bool> {
        let Some(session_id) = &self.session_id else {
            return Ok(true);
        };

        let session = match self.agent.load_session(session_id).await? {
            Some(session) => session,
            None => return Ok(true),
        };
        let count = session.transcript().await.len();
        if count <= 4 {
            return Ok(true);
        }

        print!("{DIM}  clear current conversation ({count} messages)? [y/N] {RESET}");
        std::io::stdout().flush()?;

        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        let answer = answer.trim().to_lowercase();
        Ok(answer == "y" || answer == "yes")
    }

    fn print_banner(&self) -> Result<()> {
        println!("{BOLD}Bendclaw{RESET}");
        println!(
            "{DIM}  provider: {}{RESET}",
            self.config.active_llm().provider
        );
        println!("{DIM}  model: {}{RESET}", self.config.active_llm().model);
        if let Some(base_url) = &self.config.active_llm().base_url {
            println!("{DIM}  base_url: {base_url}{RESET}");
        }
        if let Some(branch) = git_branch() {
            println!("{DIM}  git:   {branch}{RESET}");
        }
        println!("{DIM}  cwd:   {}{RESET}", self.cwd);
        println!(
            "{DIM}  prompt: /help for commands  ·  Tab for completion  ·  Ctrl+D to exit{RESET}\n"
        );
        Ok(())
    }

    async fn print_resume_hint(&self) -> Result<()> {
        let sessions = self.agent.list_sessions(20).await?;
        if let Some(session) = sessions.into_iter().find(|session| session.cwd == self.cwd) {
            println!(
                "{DIM}  previous session found. Use {YELLOW}/resume {}{RESET}{DIM} to continue.{RESET}\n",
                short_id(&session.session_id),
            );
        }
        Ok(())
    }

    fn print_resume_hint_on_exit(&self) {
        if let Some(session_id) = &self.session_id {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| "bendclaw".to_string());
            let rule = "─".repeat(80);
            println!("\n{DIM}{rule}{RESET}");
            println!("{DIM}Resume this session with:{RESET}\n  {exe} --resume {session_id}\n");
        }
    }

    async fn print_status(&self) -> Result<()> {
        let active = self.config.active_llm();
        println!("{BOLD}Status{RESET}");
        println!("{DIM}  provider:{RESET} {}", active.provider);
        println!("{DIM}  model:{RESET}    {}", active.model);
        println!(
            "{DIM}  session:{RESET}  {}",
            self.session_id
                .as_deref()
                .map(short_id)
                .unwrap_or_else(|| "(new)".into())
        );
        if let Some(branch) = git_branch() {
            println!("{DIM}  git:{RESET}      {branch}");
        }
        println!("{DIM}  cwd:{RESET}      {}", self.cwd);

        if let Some(meta) = self.current_session_meta().await? {
            println!(
                "{DIM}  title:{RESET}    {}",
                meta.title.unwrap_or_else(|| "(untitled)".into())
            );
            println!("{DIM}  turns:{RESET}    {}", meta.turns);
            println!(
                "{DIM}  updated:{RESET}  {}",
                relative_time(&meta.updated_at)
            );
        }

        println!();
        Ok(())
    }

    fn print_config(&self) {
        let active = self.config.active_llm();
        println!("{BOLD}Config{RESET}");
        println!("{DIM}  active provider:{RESET} {}", active.provider);
        println!("{DIM}  active model:{RESET}    {}", active.model);
        println!(
            "{DIM}  anthropic:{RESET}       {}",
            self.config.anthropic.model
        );
        println!(
            "{DIM}  openai:{RESET}          {}",
            self.config.openai.model
        );
        println!();
    }

    fn print_version(&self) {
        println!(
            "bendclaw {}  ({})",
            env!("CARGO_PKG_VERSION"),
            &env!("BENDCLAW_GIT_SHA")[..env!("BENDCLAW_GIT_SHA").len().min(8)]
        );
        println!("{DIM}  branch: {}{RESET}", env!("BENDCLAW_GIT_BRANCH"));
        println!(
            "{DIM}  built:  {}{RESET}\n",
            env!("BENDCLAW_BUILD_TIMESTAMP")
        );
    }

    async fn print_history(&self) -> Result<()> {
        let Some(session_id) = &self.session_id else {
            println!("{DIM}  no active session{RESET}\n");
            return Ok(());
        };

        let session =
            self.agent.load_session(session_id).await?.ok_or_else(|| {
                BendclawError::Session(format!("session not found: {session_id}"))
            })?;
        let messages = session.transcript().await;

        if messages.is_empty() {
            println!("{DIM}  session is empty{RESET}\n");
            return Ok(());
        }

        print_transcript_messages(&messages);
        Ok(())
    }

    async fn choose_session(&mut self, include_all: bool) -> Result<()> {
        let filtered = self.list_selectable_sessions(include_all).await?;
        if filtered.is_empty() {
            println!("{DIM}  no sessions found{RESET}\n");
            return Ok(());
        }

        let options = filtered
            .iter()
            .map(|session| {
                let scope_marker = if session.cwd == self.cwd { "*" } else { " " };
                SelectorOption {
                    id: session.session_id.clone(),
                    primary: format!(
                        "{}  {}  {} turns  {}",
                        short_id(&session.session_id),
                        relative_time(&session.updated_at),
                        session.turns,
                        session.model
                    ),
                    secondary: format!(
                        "{scope_marker} {}",
                        summarize_title(session.title.as_deref().unwrap_or("Untitled session"))
                    ),
                }
            })
            .collect::<Vec<_>>();
        let selected = self
            .session_id
            .as_ref()
            .and_then(|session_id| options.iter().position(|option| option.id == *session_id));
        let title = if include_all {
            "Sessions · All"
        } else {
            "Sessions · Current Folder"
        };

        let Some(index) = run_selector(
            title,
            "Type to filter sessions...",
            "↑↓ navigate  ·  Type filter  ·  Enter resume  ·  Esc cancel",
            &options,
            selected,
        )?
        else {
            println!("{DIM}  (resume cancelled){RESET}\n");
            return Ok(());
        };
        self.resume_session(&options[index].id, true).await?;
        Ok(())
    }

    async fn list_selectable_sessions(&self, include_all: bool) -> Result<Vec<SessionMeta>> {
        let sessions = self.agent.list_sessions(20).await?;
        let filtered = if include_all {
            sessions
        } else {
            let scoped: Vec<_> = sessions
                .iter()
                .filter(|session| session.cwd == self.cwd)
                .cloned()
                .collect();
            if scoped.is_empty() {
                sessions
            } else {
                scoped
            }
        };

        Ok(filtered)
    }

    fn print_help_summary(&self) {
        println!("{BOLD}Commands{RESET}");
        for command in KNOWN_COMMANDS {
            let name = &command[1..];
            let summary = command_short_description(name).unwrap_or("");
            println!("  {YELLOW}{command:<10}{RESET} {summary}");
        }
        println!();
    }

    fn print_help_for(&self, command: &str) {
        let command = command.trim().trim_start_matches('/');
        match command_help(command) {
            Some(text) => println!("{text}\n"),
            None => {
                eprintln!("{RED}  unknown command: /{command}{RESET}");
                eprintln!("{DIM}  type /help for available commands{RESET}\n");
            }
        }
    }

    fn print_model(&self) {
        println!("current model: {}\n", self.config.active_llm().model);
    }

    async fn choose_model(&mut self) -> Result<()> {
        let models = available_models(&self.config);
        if models.is_empty() {
            println!("{DIM}  no configured model available{RESET}\n");
            return Ok(());
        }

        let current = self.config.active_llm().model;
        let options = models
            .iter()
            .map(|model| SelectorOption {
                id: model.clone(),
                primary: model.clone(),
                secondary: provider_marker_for_model(&self.config, model).to_string(),
            })
            .collect::<Vec<_>>();
        let selected = options.iter().position(|option| option.id == current);

        let Some(index) = run_selector(
            "Models",
            "Type to filter models...",
            "↑↓ navigate  ·  Type filter  ·  Enter select  ·  Esc cancel",
            &options,
            selected,
        )?
        else {
            println!("{DIM}  (model switch cancelled){RESET}\n");
            return Ok(());
        };
        self.set_model(&options[index].id)?;
        Ok(())
    }

    fn set_model(&mut self, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            self.print_model();
            return Ok(());
        }

        let new_provider =
            if self.config.openai.model == value && self.config.anthropic.model != value {
                Some(ProviderKind::OpenAi)
            } else if self.config.anthropic.model == value && self.config.openai.model != value {
                Some(ProviderKind::Anthropic)
            } else {
                None
            };

        if let Some(provider) = new_provider {
            self.config.llm.provider = provider;
        }

        let provider = self.config.llm.provider.clone();
        self.config.provider_config_mut(&provider).model = value.to_string();
        self.agent.set_llm(self.config.active_llm());
        println!(
            "{DIM}  model -> {value}  ·  provider {}{RESET}\n",
            self.config.llm.provider
        );
        Ok(())
    }

    fn print_provider(&self) {
        println!("current provider: {}\n", self.config.llm.provider);
    }

    fn set_provider(&mut self, value: &str) -> Result<()> {
        let value = value.trim();
        if value.is_empty() {
            self.print_provider();
            return Ok(());
        }

        let provider = ProviderKind::from_str_loose(value)?;
        self.config.llm.provider = provider;
        self.agent.set_llm(self.config.active_llm());
        println!(
            "{DIM}  provider -> {}  ·  model {}{RESET}\n",
            self.config.llm.provider,
            self.config.active_llm().model
        );
        Ok(())
    }

    async fn resolve_session_id(&self, value: &str) -> Result<String> {
        let value = value.trim();
        if value.is_empty() {
            return Err(BendclawError::Cli("missing session id".into()));
        }

        let sessions = self.agent.list_sessions(100).await?;
        let matches: Vec<_> = sessions
            .into_iter()
            .filter(|session| session.session_id == value || session.session_id.starts_with(value))
            .collect();

        match matches.len() {
            0 => Err(BendclawError::Session(format!(
                "session not found: {value}"
            ))),
            1 => Ok(matches[0].session_id.clone()),
            _ => Err(BendclawError::Session(format!(
                "session id is ambiguous: {value}"
            ))),
        }
    }

    async fn current_session_meta(&self) -> Result<Option<SessionMeta>> {
        let Some(session_id) = &self.session_id else {
            return Ok(None);
        };
        self.agent.get_session(session_id).await
    }

    async fn refresh_completion_state(&self) -> Result<()> {
        let models = available_models(&self.config);
        let sessions = self.agent.list_sessions(20).await?;
        let session_ids = sessions
            .into_iter()
            .map(|session| session.session_id)
            .collect();

        let mut state = self
            .completion_state
            .write()
            .map_err(|_| BendclawError::Cli("completion state lock poisoned".into()))?;
        state.models = models;
        state.session_ids = session_ids;
        Ok(())
    }

    fn prompt(&self) -> String {
        let branch = git_branch();
        let session = self
            .session_id
            .as_deref()
            .map(short_id)
            .unwrap_or_else(|| "new".into());

        match branch {
            Some(branch) => format!(
                "{BOLD}{GREEN}{}{RESET} {DIM}[{}]{RESET} {BOLD}{YELLOW}>{RESET}",
                branch, session,
            ),
            None => format!("{DIM}[{}]{RESET} {BOLD}{YELLOW}>{RESET}", session,),
        }
    }

    fn load_history(&self, rl: &mut Editor<ReplHelper, DefaultHistory>) {
        let Ok(path) = paths::history_file_path() else {
            return;
        };
        let _ = rl.load_history(&path);
    }

    fn save_history(&self, rl: &mut Editor<ReplHelper, DefaultHistory>) {
        let Ok(path) = paths::history_file_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = rl.save_history(&path);
    }
}

// ---------------------------------------------------------------------------
// Multiline input
// ---------------------------------------------------------------------------

pub fn needs_continuation(line: &str) -> bool {
    line.ends_with('\\') || line.starts_with("```")
}

pub fn collect_multiline_rl(
    first_line: &str,
    rl: &mut Editor<ReplHelper, DefaultHistory>,
) -> String {
    let mut buf = String::new();
    let cont_prompt = format!("{DIM}  ...{RESET} ");

    if first_line.starts_with("```") {
        buf.push_str(first_line);
        buf.push('\n');
        while let Ok(line) = rl.readline(&cont_prompt) {
            buf.push_str(&line);
            buf.push('\n');
            if line.trim() == "```" {
                break;
            }
        }
    } else {
        let mut current = first_line.to_string();
        loop {
            if current.ends_with('\\') {
                current.truncate(current.len() - 1);
                buf.push_str(&current);
                buf.push('\n');
                match rl.readline(&cont_prompt) {
                    Ok(line) => current = line,
                    Err(_) => break,
                }
            } else {
                buf.push_str(&current);
                break;
            }
        }
    }

    buf
}

// ---------------------------------------------------------------------------
// Utilities
// ---------------------------------------------------------------------------

fn git_branch() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() || branch == "HEAD" {
        None
    } else {
        Some(branch)
    }
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn summarize_title(value: &str) -> String {
    truncate(value, 56)
}

fn relative_time(value: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(datetime) => {
            let duration =
                chrono::Utc::now().signed_duration_since(datetime.with_timezone(&chrono::Utc));
            if duration.num_minutes() <= 0 {
                "just now".into()
            } else if duration.num_hours() <= 0 {
                format!("{}m ago", duration.num_minutes())
            } else if duration.num_days() <= 0 {
                format!("{}h ago", duration.num_hours())
            } else {
                format!("{}d ago", duration.num_days())
            }
        }
        Err(_) => value.into(),
    }
}
