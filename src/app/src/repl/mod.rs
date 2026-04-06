use std::borrow::Cow;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::thread::JoinHandle;
use std::time::Duration;

use async_trait::async_trait;
use bend_agent::types::extract_text;
use bend_agent::types::ToolResultContentBlock;
use crossterm::cursor::Hide;
use crossterm::cursor::Show;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Clear as ClearWidget;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Row;
use ratatui::widgets::Table;
use ratatui::Terminal;
use rustyline::completion::Completer;
use rustyline::completion::Pair;
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::Editor;

use crate::conf::paths;
use crate::conf::Config;
use crate::conf::ProviderKind;
use crate::error::BendclawError;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::EventSink;
use crate::request::MessagePayload;
use crate::request::Request;
use crate::request::RequestExecutor;
use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::session::Session;
use crate::storage::model::ListSessions;
use crate::storage::model::RunEvent;
use crate::storage::model::RunEventKind;
use crate::storage::model::SessionMeta;
use crate::storage::Storage;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const BLACK: &str = "\x1b[30m";
const WHITE: &str = "\x1b[37m";
const GRAY: &str = "\x1b[90m";
const BG_TOOL: &str = "\x1b[48;2;245;197;66m";
const BG_OK: &str = "\x1b[48;2;133;220;140m";
const BG_ERR: &str = "\x1b[48;2;157;57;57m";
const CLEAR_LINE: &str = "\r\x1b[2K\r";
const MAX_SELECTOR_ROWS: usize = 12;

const KNOWN_COMMANDS: &[&str] = &[
    "/help",
    "/status",
    "/config",
    "/history",
    "/sessions",
    "/resume",
    "/new",
    "/clear",
    "/clear!",
    "/model",
    "/provider",
    "/version",
    "/quit",
    "/exit",
];

type CompletionStateRef = Arc<RwLock<CompletionState>>;

#[derive(Default)]
struct CompletionState {
    models: Vec<String>,
    session_ids: Vec<String>,
}

pub struct Repl {
    config: Config,
    storage: Arc<dyn Storage>,
    max_turns: Option<u32>,
    append_system_prompt: Option<String>,
    session_id: Option<String>,
    cwd: String,
    completion_state: CompletionStateRef,
}

impl Repl {
    pub fn new(
        config: Config,
        storage: Arc<dyn Storage>,
        max_turns: Option<u32>,
        append_system_prompt: Option<String>,
        session_id: Option<String>,
    ) -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|e| BendclawError::Cli(format!("failed to get cwd: {e}")))?
            .to_string_lossy()
            .to_string();

        Ok(Self {
            config,
            storage,
            max_turns,
            append_system_prompt,
            session_id,
            cwd,
            completion_state: Arc::new(RwLock::new(CompletionState::default())),
        })
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
        println!("\n{DIM}  bye{RESET}\n");
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
        let mut request = Request::new(input.to_string());
        request.session_id = self.session_id.clone();
        request.max_turns = self.max_turns;
        request.append_system_prompt = self.append_system_prompt.clone();

        let sink = Arc::new(ReplSink::default());
        let runner = crate::request::RequestRunner::new();
        let executor = RequestExecutor::new(
            request,
            self.config.active_llm(),
            sink,
            self.storage.clone(),
            runner.clone(),
        );

        let mut run_task = tokio::spawn(async move { executor.execute().await });
        let control = wait_for_run_control(&mut run_task)?;
        let outcome = match control {
            Some(action) => {
                runner.close().await;
                run_task.abort();
                let _ = run_task.await;
                PromptExit::Cancelled(action == RunControl::Exit)
            }
            None => {
                let result = run_task
                    .await
                    .map_err(|e| BendclawError::Cli(format!("request task failed: {e}")))??;
                PromptExit::Finished(result, false)
            }
        };

        match outcome {
            PromptExit::Finished(result, exit_requested) => {
                self.session_id = Some(result.session_id);
                Ok(exit_requested)
            }
            PromptExit::Cancelled(exit_requested) => {
                println!("{DIM}[stopped]{RESET}");
                Ok(exit_requested)
            }
        }
    }

    async fn resume_session(&mut self, session_id: &str, print_transcript: bool) -> Result<()> {
        let session = Session::load(session_id, self.storage.clone())
            .await?
            .ok_or_else(|| BendclawError::Session(format!("session not found: {session_id}")))?;
        let meta = session.meta().await;
        let messages = session.messages().await;
        let provider = self.config.llm.provider.clone();

        self.session_id = Some(meta.session_id.clone());
        self.config.provider_config_mut(&provider).model = meta.model.clone();

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

        let session = match Session::load(session_id, self.storage.clone()).await? {
            Some(session) => session,
            None => return Ok(true),
        };
        let count = session.messages().await.len();
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
        let sessions = self
            .storage
            .list_sessions(ListSessions { limit: 20 })
            .await?;
        if let Some(session) = sessions.into_iter().find(|session| session.cwd == self.cwd) {
            println!(
                "{DIM}  previous session found. Use {YELLOW}/resume {}{RESET}{DIM} to continue.{RESET}\n",
                short_id(&session.session_id),
            );
        }
        Ok(())
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

        let session = Session::load(session_id, self.storage.clone())
            .await?
            .ok_or_else(|| BendclawError::Session(format!("session not found: {session_id}")))?;
        let messages = session.messages().await;

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
        let sessions = self
            .storage
            .list_sessions(ListSessions { limit: 20 })
            .await?;
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

        let provider = self.config.llm.provider.clone();
        self.config.provider_config_mut(&provider).model = value.to_string();
        println!("{DIM}  model -> {value}{RESET}\n");
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

        let sessions = self
            .storage
            .list_sessions(ListSessions { limit: 100 })
            .await?;
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
        self.storage.get_session(session_id).await
    }

    async fn refresh_completion_state(&self) -> Result<()> {
        let models = available_models(&self.config);
        let sessions = self
            .storage
            .list_sessions(ListSessions { limit: 20 })
            .await?;
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

#[derive(Default)]
struct SinkState {
    assistant_open: bool,
    assistant_prefixed: bool,
    streamed_assistant: bool,
    pending_tools: HashMap<String, ToolCallDisplay>,
    spinner: Option<Spinner>,
}

struct ToolCallDisplay {
    name: String,
    summary: String,
}

#[derive(Default)]
struct ReplSink {
    state: Mutex<SinkState>,
}

#[async_trait]
impl EventSink for ReplSink {
    async fn publish(&self, event: Arc<RunEvent>) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| BendclawError::Cli("sink state lock poisoned".into()))?;

        match &event.kind {
            RunEventKind::RunStarted => {
                state.assistant_open = false;
                state.assistant_prefixed = false;
                state.streamed_assistant = false;
                state.spinner = Some(Spinner::start("Thinking..."));
            }
            RunEventKind::AssistantMessage => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                    for block in payload.content {
                        match block {
                            AssistantBlock::Text { text } => {
                                if state.streamed_assistant {
                                    terminal_writeln("");
                                } else if !text.trim().is_empty() {
                                    terminal_prefixed_writeln(&text);
                                }
                                state.assistant_open = false;
                                state.assistant_prefixed = false;
                                state.streamed_assistant = false;
                            }
                            AssistantBlock::ToolUse { id, name, input } => {
                                finish_assistant_line(&mut state);
                                state.pending_tools.insert(id, ToolCallDisplay {
                                    name: name.clone(),
                                    summary: format_tool_input(&input),
                                });
                                print_tool_call(&name, &input);
                            }
                            AssistantBlock::Thinking { .. } => {}
                        }
                    }
                }
            }
            RunEventKind::ToolResult => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let tool_call = state.pending_tools.remove(&payload.tool_use_id);
                    print_tool_result(&payload, tool_call.as_ref());
                }
            }
            RunEventKind::PartialMessage => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    if !state.assistant_prefixed {
                        terminal_message_prefix();
                        state.assistant_prefixed = true;
                    }
                    terminal_write(&payload.message);
                    state.assistant_open = true;
                    state.streamed_assistant = true;
                }
            }
            RunEventKind::CompactBoundary => {
                stop_spinner(&mut state);
                finish_assistant_line(&mut state);
                if let Some(summary) = event
                    .payload
                    .get("summary")
                    .and_then(|value| value.as_str())
                {
                    terminal_writeln(&format!("[compact] {}", truncate(summary, 120)));
                }
            }
            RunEventKind::Status => {
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    update_spinner(&mut state, &payload.message);
                }
            }
            RunEventKind::System => {}
            RunEventKind::TaskNotification => {
                if let Some(message) = event
                    .payload
                    .get("message")
                    .and_then(|value| value.as_str())
                {
                    update_spinner(&mut state, message);
                }
            }
            RunEventKind::RateLimit => {
                stop_spinner(&mut state);
                if let Some(message) = event
                    .payload
                    .get("message")
                    .and_then(|value| value.as_str())
                {
                    finish_assistant_line(&mut state);
                    terminal_writeln(&format!("{YELLOW}[rate-limit] {}{RESET}", message));
                }
            }
            RunEventKind::Progress => {
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    update_spinner(&mut state, &payload.message);
                }
            }
            RunEventKind::Error => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    terminal_writeln(&format!("{RED}error:{RESET} {}", payload.message));
                }
            }
            RunEventKind::RunFinished => {
                stop_spinner(&mut state);
                if let Some(payload) = payload_as::<RequestFinishedPayload>(&event.payload) {
                    finish_assistant_line(&mut state);
                    let summary = build_run_summary(&payload);
                    if !summary.is_empty() {
                        terminal_writeln(&format!("{DIM}{summary}{RESET}"));
                    }
                }
            }
        }

        Ok(())
    }
}

fn finish_assistant_line(state: &mut SinkState) {
    if state.assistant_open {
        terminal_writeln("");
    }
    state.assistant_open = false;
    state.assistant_prefixed = false;
    state.streamed_assistant = false;
}

fn stop_spinner(state: &mut SinkState) {
    if let Some(mut spinner) = state.spinner.take() {
        spinner.stop();
    }
}

fn update_spinner(state: &mut SinkState, message: &str) {
    if let Some(spinner) = state.spinner.as_mut() {
        spinner.update(message);
    } else {
        state.spinner = Some(Spinner::start(message));
    }
}

struct Spinner {
    running: Arc<AtomicBool>,
    message: Arc<Mutex<String>>,
    handle: Option<JoinHandle<()>>,
}

impl Spinner {
    fn start(initial_message: &str) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let message = Arc::new(Mutex::new(initial_message.to_string()));
        let running_flag = running.clone();
        let message_ref = message.clone();

        let handle = std::thread::spawn(move || {
            let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
            let mut index = 0usize;
            while running_flag.load(Ordering::Relaxed) {
                let label = message_ref
                    .lock()
                    .map(|value| value.clone())
                    .unwrap_or_else(|_| "Working...".into());
                print!(
                    "{CLEAR_LINE}{DIM}{} {}{RESET}",
                    frames[index % frames.len()],
                    label
                );
                let _ = std::io::stdout().flush();
                std::thread::sleep(Duration::from_millis(80));
                index = index.wrapping_add(1);
            }
            print!("{CLEAR_LINE}");
            let _ = std::io::stdout().flush();
        });

        Self {
            running,
            message,
            handle: Some(handle),
        }
    }

    fn update(&mut self, message: &str) {
        if let Ok(mut current) = self.message.lock() {
            *current = message.to_string();
        }
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Spinner {
    fn drop(&mut self) {
        self.stop();
    }
}

fn print_transcript_messages(messages: &[bend_agent::Message]) {
    for message in messages {
        match message.role {
            bend_agent::MessageRole::User => {
                let text = extract_text(message);
                if !text.trim().is_empty() {
                    println!("{YELLOW}> {RESET}{}", text.trim());
                    println!();
                }
                for block in &message.content {
                    if let bend_agent::ContentBlock::ToolResult {
                        content, is_error, ..
                    } = block
                    {
                        print_tool_result_content("tool result", content, !*is_error);
                    }
                }
            }
            bend_agent::MessageRole::Assistant => {
                let text = extract_text(message);
                if !text.trim().is_empty() {
                    terminal_prefixed_writeln(text.trim());
                    terminal_writeln("");
                }
                for block in &message.content {
                    match block {
                        bend_agent::ContentBlock::ToolUse { name, input, .. } => {
                            print_tool_call(name, input);
                        }
                        bend_agent::ContentBlock::ToolResult {
                            content, is_error, ..
                        } => {
                            print_tool_result_content("tool result", content, !*is_error);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn print_tool_call(name: &str, input: &serde_json::Value) {
    let (title, lines) = tool_call_message(name, input);
    print_badge_line(&title, false, false);
    for line in lines {
        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
    }
    terminal_writeln("");
}

fn print_tool_result(payload: &ToolResultPayload, tool_call: Option<&ToolCallDisplay>) {
    let title = tool_result_title(payload);
    let line = tool_result_line(payload, tool_call);
    print_badge_line(&title, true, !payload.is_error);
    terminal_writeln(&format!(
        "{}  {}{}",
        if payload.is_error { RED } else { GREEN },
        line,
        RESET
    ));
    terminal_writeln("");
}

fn print_badge_line(title: &str, is_result: bool, ok: bool) {
    let (badge, rest) = split_tool_title(title);
    let (fg, bg) = if is_result {
        if ok {
            (BLACK, BG_OK)
        } else {
            (WHITE, BG_ERR)
        }
    } else {
        (BLACK, BG_TOOL)
    };

    if rest.is_empty() {
        terminal_writeln(&format!("{bg}{fg}{BOLD}[{badge}]{RESET}"));
    } else {
        terminal_writeln(&format!(
            "{bg}{fg}{BOLD}[{badge}]{RESET} {GRAY}{rest}{RESET}"
        ));
    }
}

fn print_tool_result_content(title: &str, content: &[ToolResultContentBlock], ok: bool) {
    let text_blocks: Vec<_> = content
        .iter()
        .filter_map(|block| match block {
            ToolResultContentBlock::Text { text } => Some(text.as_str()),
            ToolResultContentBlock::Image { .. } => None,
        })
        .collect();
    if text_blocks.is_empty() {
        terminal_writeln(&format!(
            "[{}] {}",
            if ok { "done" } else { "error" },
            title
        ));
    } else {
        terminal_writeln(&format!(
            "[{}] {}",
            if ok { "done" } else { "error" },
            truncate(&text_blocks.join("\n"), 160)
        ));
    }
    terminal_writeln("");
}

fn tool_call_message(name: &str, input: &serde_json::Value) -> (String, Vec<String>) {
    let lowercase = name.to_lowercase();
    if lowercase.contains("grep") {
        return ("Grep 1 search".into(), vec![format!(
            "\"{}\"",
            format_tool_input(input)
        )]);
    }
    if lowercase.contains("glob") {
        return ("Glob 1 pattern".into(), vec![format_tool_input(input)]);
    }
    if lowercase.contains("read") {
        return ("Read 1 file".into(), vec![format_tool_input(input)]);
    }

    (format!("{name} call"), vec![format_tool_input(input)])
}

fn tool_result_title(payload: &ToolResultPayload) -> String {
    if payload.is_error {
        format!("{} failed", payload.tool_name)
    } else {
        format!("{} completed", payload.tool_name)
    }
}

fn tool_result_line(payload: &ToolResultPayload, tool_call: Option<&ToolCallDisplay>) -> String {
    if !payload.is_error {
        if let Some(tool_call) = tool_call {
            if tool_call.name.to_lowercase().contains("read") {
                return format!("Result: {}", tool_call.summary);
            }
        }
    }

    if payload.content.trim().is_empty() {
        if payload.is_error {
            "Result: tool returned an error".into()
        } else {
            "Result: completed".into()
        }
    } else {
        format!("Result: {}", summarize_inline(&payload.content, 160))
    }
}

fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
}

fn summarize_inline(value: &str, max_chars: usize) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    truncate(&collapsed, max_chars)
}

fn build_run_summary(payload: &RequestFinishedPayload) -> String {
    let summary = serde_json::from_value::<bend_agent::RunSummary>(payload.summary.clone())
        .unwrap_or_default();
    let total_tokens = payload
        .usage
        .get("input_tokens")
        .and_then(|value| value.as_u64())
        .unwrap_or_default()
        + payload
            .usage
            .get("output_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();

    let mut parts = vec![
        format!("run {}", human_duration(payload.duration_ms)),
        format!("turns {}", payload.num_turns),
        format!("tokens {}", total_tokens),
    ];
    if summary.api_duration_ms > 0 {
        parts.push(format!("llm {}", human_duration(summary.api_duration_ms)));
    }
    if summary.tool_duration_ms > 0 {
        parts.push(format!(
            "tools {}",
            human_duration(summary.tool_duration_ms)
        ));
    }
    parts.join("  ·  ")
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

fn normalize_terminal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

fn terminal_write(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    print!("{normalized}");
    let _ = std::io::stdout().flush();
}

fn terminal_writeln(text: &str) {
    terminal_write(text);
    terminal_write("\r\n");
}

fn terminal_message_prefix() {
    terminal_write(&format!("{DIM}•{RESET} "));
}

fn terminal_prefixed_writeln(text: &str) {
    terminal_message_prefix();
    terminal_writeln(text);
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut value: String = s.chars().take(max).collect();
        value.push_str("...");
        value
    }
}

const SUMMARY_KEYS: &[&str] = &[
    "file_path",
    "path",
    "command",
    "pattern",
    "patterns",
    "query",
    "url",
    "name",
    "directory",
    "glob",
    "regex",
];

fn format_tool_input(input: &serde_json::Value) -> String {
    if let Some(obj) = input.as_object() {
        for &key in SUMMARY_KEYS {
            if let Some(val) = obj.get(key) {
                if let Some(s) = val.as_str() {
                    return summarize_inline(s, 100);
                }
                if let Some(arr) = val.as_array() {
                    let parts: Vec<&str> = arr.iter().filter_map(|v| v.as_str()).collect();
                    if !parts.is_empty() {
                        return summarize_inline(&parts.join(", "), 100);
                    }
                }
            }
        }
    }
    summarize_inline(&input.to_string(), 100)
}

fn available_models(config: &Config) -> Vec<String> {
    let mut models = Vec::new();
    for model in [
        config.anthropic.model.clone(),
        config.openai.model.clone(),
        config.active_llm().model,
    ] {
        if !model.trim().is_empty() && !models.contains(&model) {
            models.push(model);
        }
    }
    models
}

fn provider_marker_for_model(config: &Config, model: &str) -> &'static str {
    if config.anthropic.model == model && config.openai.model == model {
        "anthropic/openai"
    } else if config.anthropic.model == model {
        "anthropic"
    } else if config.openai.model == model {
        "openai"
    } else {
        "custom"
    }
}

struct SelectorOption {
    id: String,
    primary: String,
    secondary: String,
}

struct SelectorPopup<'a> {
    title: &'a str,
    placeholder: &'a str,
    footer: &'a str,
    options: &'a [SelectorOption],
    filtered: &'a [usize],
    filter: &'a str,
    selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RunControl {
    Cancel,
    Exit,
}

enum PromptExit {
    Finished(crate::request::RequestResult, bool),
    Cancelled(bool),
}

fn run_selector(
    title: &str,
    placeholder: &str,
    footer: &str,
    options: &[SelectorOption],
    selected: Option<usize>,
) -> Result<Option<usize>> {
    let mut terminal = SelectorTerminal::enter()?;
    let mut filter = String::new();
    let mut selected = selected.unwrap_or(0);

    loop {
        let filtered = filtered_selector_indices(options, &filter);
        if filtered.is_empty() {
            selected = 0;
        } else if selected >= filtered.len() {
            selected = filtered.len() - 1;
        }

        let popup = SelectorPopup {
            title,
            placeholder,
            footer,
            options,
            filtered: &filtered,
            filter: &filter,
            selected,
        };

        terminal.draw(|frame| {
            render_selector_popup(frame, &popup);
        })?;

        let event = read()?;
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => return Ok(filtered.get(selected).copied()),
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    if !filtered.is_empty() && selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                KeyCode::Char(ch) => {
                    filter.push(ch);
                    selected = 0;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn filtered_selector_indices(options: &[SelectorOption], filter: &str) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, option)| {
            filter.is_empty()
                || option.primary.to_lowercase().contains(&filter)
                || option.secondary.to_lowercase().contains(&filter)
                || option.id.to_lowercase().contains(&filter)
        })
        .map(|(index, _)| index)
        .collect()
}

fn render_selector_popup(frame: &mut ratatui::Frame<'_>, popup: &SelectorPopup<'_>) {
    let area = selector_area(frame.area(), popup.filtered.len());
    frame.render_widget(ClearWidget, area);

    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" {} ", popup.title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    render_selector_filter(frame, parts[0], popup.placeholder, popup.filter);

    if popup.filtered.is_empty() {
        frame.render_widget(
            Paragraph::new("no matches")
                .alignment(Alignment::Left)
                .style(Style::default().fg(Color::DarkGray)),
            parts[1],
        );
    } else {
        let start = selector_scroll_start(popup.selected, popup.filtered.len());
        let rows = popup
            .filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(MAX_SELECTOR_ROWS)
            .map(|(visible_index, option_index)| {
                let option = &popup.options[*option_index];
                let selected_row = visible_index == popup.selected;
                let row_style = if selected_row {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(50, 50, 50))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let marker = if selected_row { "●" } else { " " };

                Row::new(vec![
                    Cell::from(Span::styled(marker, Style::default().fg(Color::Yellow))),
                    Cell::from(option.primary.clone()),
                    Cell::from(option.secondary.clone()),
                ])
                .style(row_style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Table::new(rows, [
                Constraint::Length(2),
                Constraint::Percentage(46),
                Constraint::Min(18),
            ])
            .column_spacing(1),
            parts[1],
        );
    }

    frame.render_widget(
        Paragraph::new(format!("{}   {} items", popup.footer, popup.filtered.len()))
            .style(Style::default().fg(Color::DarkGray)),
        parts[2],
    );
}

fn render_selector_filter(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    placeholder: &str,
    filter: &str,
) {
    let text = if filter.is_empty() {
        placeholder
    } else {
        filter
    };
    let style = if filter.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("█", Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(text.to_string(), style),
        ])),
        area,
    );
}

fn selector_scroll_start(selected: usize, len: usize) -> usize {
    if len <= MAX_SELECTOR_ROWS {
        0
    } else if selected >= MAX_SELECTOR_ROWS {
        selected + 1 - MAX_SELECTOR_ROWS
    } else {
        0
    }
}

fn selector_area(frame_area: Rect, item_count: usize) -> Rect {
    let rows = item_count.clamp(1, MAX_SELECTOR_ROWS) as u16;
    let width = frame_area
        .width
        .saturating_mul(88)
        .saturating_div(100)
        .max(56);
    let height = (rows + 6).min(frame_area.height.saturating_sub(2)).max(8);
    centered_rect(frame_area, width.min(frame_area.width), height)
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(width),
            Constraint::Fill(1),
        ])
        .split(vertical[1]);
    horizontal[1]
}

struct SelectorTerminal {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl SelectorTerminal {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)
            .map_err(|e| BendclawError::Cli(format!("failed to initialize selector: {e}")))?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, render: F) -> Result<()>
    where F: FnOnce(&mut ratatui::Frame<'_>) {
        self.terminal
            .draw(render)
            .map_err(|e| BendclawError::Cli(format!("failed to draw selector: {e}")))?;
        Ok(())
    }
}

impl Drop for SelectorTerminal {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

fn wait_for_run_control(
    run_task: &mut tokio::task::JoinHandle<Result<crate::request::RequestResult>>,
) -> Result<Option<RunControl>> {
    let _guard = RawModeGuard::enter()?;
    loop {
        if run_task.is_finished() {
            return Ok(None);
        }
        if !poll(Duration::from_millis(50))? {
            continue;
        }
        match read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(Some(RunControl::Cancel)),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Some(RunControl::Exit))
                }
                _ => {}
            },
            _ => {}
        }
    }
}

struct RawModeGuard;

impl RawModeGuard {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

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

fn command_short_description(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some("show help"),
        "status" => Some("show current session status"),
        "config" => Some("show provider/model config"),
        "history" => Some("print current transcript"),
        "sessions" => Some("choose a recent session"),
        "resume" => Some("resume a session"),
        "new" => Some("start a new session"),
        "clear" => Some("clear conversation"),
        "clear!" => Some("clear without confirmation"),
        "model" => Some("show or change model"),
        "provider" => Some("show or change provider"),
        "version" => Some("show build info"),
        "quit" | "exit" => Some("exit bendclaw"),
        _ => None,
    }
}

fn command_help(cmd: &str) -> Option<&'static str> {
    match cmd {
        "help" => Some(
            "/help [command] - Show help information\n\nUsage:\n  /help\n  /help model\n  /help resume",
        ),
        "status" => Some(
            "/status - Show current provider, model, session, cwd, and session metadata.",
        ),
        "config" => Some(
            "/config - Show the active provider/model and the configured provider defaults.",
        ),
        "history" => Some(
            "/history - Print the current session transcript from storage.",
        ),
        "sessions" => Some(
            "/sessions [all] - List recent sessions and let you choose one.\n\nDefault scope is current folder when matches exist. Use `/sessions all` to show everything.",
        ),
        "resume" => Some(
            "/resume [session-id] - Resume a previous session.\n\nWithout an argument it opens the session selector. Prefixes are accepted when unambiguous.",
        ),
        "new" => Some(
            "/new - Start a fresh session without deleting stored history.",
        ),
        "clear" => Some(
            "/clear - Start a fresh session after confirmation when the current transcript is non-trivial.\n\nSee also: /clear!",
        ),
        "clear!" => Some(
            "/clear! - Force a fresh session without confirmation.",
        ),
        "model" => Some(
            "/model [name] - Show or change the active model for the current provider.\n\nWithout an argument it opens the model selector.",
        ),
        "provider" => Some(
            "/provider [anthropic|openai] - Show or change the active provider.",
        ),
        "version" => Some(
            "/version - Show build version, git sha, branch, and build timestamp.",
        ),
        "quit" | "exit" => Some(
            "/quit - Exit Bendclaw.\n\nAliases: /quit, /exit",
        ),
        _ => None,
    }
}

fn help_command_completions(partial_lower: &str) -> Vec<String> {
    KNOWN_COMMANDS
        .iter()
        .map(|c| c.trim_start_matches('/'))
        .filter(|name| *name != "exit")
        .filter(|name| name.to_lowercase().starts_with(partial_lower))
        .map(|name| name.to_string())
        .collect()
}

fn command_arg_completions(cmd: &str, arg_part: &str, state: &CompletionState) -> Vec<String> {
    let partial = arg_part.to_lowercase();
    match cmd {
        "/help" => help_command_completions(&partial),
        "/provider" => ["anthropic", "openai"]
            .into_iter()
            .filter(|name| name.starts_with(&partial))
            .map(|name| name.to_string())
            .collect(),
        "/model" => state
            .models
            .iter()
            .filter(|model| model.to_lowercase().starts_with(&partial))
            .cloned()
            .collect(),
        "/resume" => state
            .session_ids
            .iter()
            .filter(|session_id| session_id.starts_with(arg_part))
            .cloned()
            .collect(),
        "/sessions" => ["all"]
            .into_iter()
            .filter(|value| value.starts_with(&partial))
            .map(|value| value.to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub struct ReplHelper {
    state: CompletionStateRef,
}

impl ReplHelper {
    fn new(state: CompletionStateRef) -> Self {
        Self { state }
    }
}

impl Completer for ReplHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];

        if prefix.starts_with('/') && !prefix.contains(' ') {
            let matches: Vec<Pair> = KNOWN_COMMANDS
                .iter()
                .filter(|cmd| cmd.starts_with(prefix))
                .map(|cmd| {
                    let cmd_name = &cmd[1..];
                    let desc = command_short_description(cmd_name).unwrap_or("");
                    if desc.is_empty() {
                        Pair {
                            display: cmd.to_string(),
                            replacement: cmd.to_string(),
                        }
                    } else {
                        Pair {
                            display: format!("{cmd:<12} {desc}"),
                            replacement: cmd.to_string(),
                        }
                    }
                })
                .collect();
            return Ok((0, matches));
        }

        if prefix.starts_with('/') {
            if let Some(space_pos) = prefix.find(' ') {
                let cmd = &prefix[..space_pos];
                let arg_part = &prefix[space_pos + 1..];
                if !arg_part.contains(' ') {
                    let state = self.state.read().map_err(|_| {
                        ReadlineError::Io(std::io::Error::other("completion state lock poisoned"))
                    })?;
                    let candidates = command_arg_completions(cmd, arg_part, &state);
                    if !candidates.is_empty() {
                        let pairs = candidates
                            .into_iter()
                            .map(|candidate| Pair {
                                display: candidate.clone(),
                                replacement: candidate,
                            })
                            .collect();
                        return Ok((space_pos + 1, pairs));
                    }
                }
            }
        }

        let word_start = prefix.rfind(char::is_whitespace).map_or(0, |i| i + 1);
        let word = &prefix[word_start..];
        if word.is_empty() {
            return Ok((pos, Vec::new()));
        }

        let matches = complete_file_path(word)
            .into_iter()
            .map(|value| Pair {
                display: value.clone(),
                replacement: value,
            })
            .collect();
        Ok((word_start, matches))
    }
}

impl Hinter for ReplHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        if pos != line.len() || !line.starts_with('/') {
            return None;
        }
        let typed = &line[1..];
        if typed.is_empty() || typed.contains(' ') {
            return None;
        }
        for cmd in KNOWN_COMMANDS {
            let cmd_name = &cmd[1..];
            if cmd_name.starts_with(typed) && cmd_name != typed {
                let rest = &cmd_name[typed.len()..];
                if let Some(desc) = command_short_description(cmd_name) {
                    return Some(format!("{rest} - {desc}"));
                }
                return Some(rest.to_string());
            }
        }
        for cmd in KNOWN_COMMANDS {
            let cmd_name = &cmd[1..];
            if cmd_name == typed {
                if let Some(desc) = command_short_description(cmd_name) {
                    return Some(format!(" - {desc}"));
                }
            }
        }
        None
    }
}

impl Highlighter for ReplHelper {
    fn highlight_hint<'h>(&self, hint: &'h str) -> Cow<'h, str> {
        Cow::Owned(format!("{DIM}{hint}{RESET}"))
    }
}

impl Validator for ReplHelper {}
impl rustyline::Helper for ReplHelper {}

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

pub fn complete_file_path(partial: &str) -> Vec<String> {
    let path = Path::new(partial);

    let (dir, file_prefix) =
        if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
            (partial.to_string(), String::new())
        } else if let Some(parent) = path.parent() {
            let parent_str = if parent.as_os_str().is_empty() {
                ".".to_string()
            } else {
                parent.to_string_lossy().to_string()
            };
            let file_prefix = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_default();
            (parent_str, file_prefix)
        } else {
            (".".to_string(), partial.to_string())
        };

    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let dir_prefix = if dir == "." && !partial.contains('/') {
        String::new()
    } else if partial.ends_with('/') || partial.ends_with(std::path::MAIN_SEPARATOR) {
        partial.to_string()
    } else {
        let parent = path.parent().unwrap_or(Path::new(""));
        if parent.as_os_str().is_empty() {
            String::new()
        } else {
            format!("{}/", parent.display())
        }
    };

    let mut matches = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with(&file_prefix) {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|value| value.is_dir())
            .unwrap_or(false);
        let candidate = if is_dir {
            format!("{}{}/", dir_prefix, name)
        } else {
            format!("{}{}", dir_prefix, name)
        };
        matches.push(candidate);
    }
    matches.sort();
    matches
}
