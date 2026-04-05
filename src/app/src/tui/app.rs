use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use ratatui::TerminalOptions;
use ratatui::Viewport;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::conf::Config;
use crate::conf::LlmConfig;
use crate::conf::ProviderKind;
use crate::error::BendclawError;
use crate::error::Result;
use crate::request::payload_as;
use crate::request::AssistantBlock;
use crate::request::AssistantPayload;
use crate::request::MessagePayload;
use crate::request::Request;
use crate::request::RequestExecutor;
use crate::request::RequestFinishedPayload;
use crate::request::ToolResultPayload;
use crate::storage::model::ListSessions;
use crate::storage::model::RunEventKind;
use crate::storage::model::SessionMeta;
use crate::storage::Storage;
use crate::tui::sink::TuiSink;
use crate::tui::state::matching_command_hints;
use crate::tui::state::MessageItem;
use crate::tui::state::ModelOption;
use crate::tui::state::PopupState;
use crate::tui::state::SessionScope;
use crate::tui::state::TuiEvent;
use crate::tui::state::TuiState;
use crate::tui::view;

pub struct Tui {
    config: Config,
    storage: Arc<dyn Storage>,
    max_turns: Option<u32>,
    append_system_prompt: Option<String>,
    session_id: Option<String>,
}

impl Tui {
    pub fn new(
        config: Config,
        storage: Arc<dyn Storage>,
        max_turns: Option<u32>,
        append_system_prompt: Option<String>,
        session_id: Option<String>,
    ) -> Arc<Self> {
        Arc::new(Self {
            config,
            storage,
            max_turns,
            append_system_prompt,
            session_id,
        })
    }

    pub async fn run(&self) -> Result<()> {
        let cwd = std::env::current_dir()
            .map_err(|e| BendclawError::Cli(format!("failed to get cwd: {e}")))?;

        let active = self.config.active_llm();
        let model = ModelOption {
            provider: active.provider.clone(),
            model: active.model.clone(),
        };
        let mut state = TuiState::new(
            cwd.to_string_lossy().to_string(),
            self.session_id.clone(),
            model,
        );
        let (tx, mut rx) = mpsc::unbounded_channel::<TuiEvent>();
        let mut running_task: Option<JoinHandle<()>> = None;

        let mut terminal = enter_terminal()?;
        let result = loop {
            if let Err(error) = terminal.draw(|frame| view::render(frame, &state)) {
                break Err(BendclawError::Cli(format!("failed to draw tui: {error}")));
            }

            while let Ok(event) = rx.try_recv() {
                self.handle_tui_event(&mut state, event);
            }

            if crossterm::event::poll(Duration::from_millis(80))
                .map_err(|e| BendclawError::Cli(format!("failed to poll input: {e}")))?
            {
                let event = crossterm::event::read()
                    .map_err(|e| BendclawError::Cli(format!("failed to read input: {e}")))?;
                match self
                    .handle_terminal_event(&mut state, event, tx.clone(), &mut running_task)
                    .await
                {
                    Ok(true) => break Ok(()),
                    Ok(false) => {}
                    Err(error) => break Err(error),
                }
            }

            if state.loading {
                state.spinner_index = state.spinner_index.wrapping_add(1);
            }
        };

        let cleanup = leave_terminal(&mut terminal);
        match (result, cleanup) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Err(error), Err(_)) => Err(error),
        }
    }

    async fn handle_terminal_event(
        &self,
        state: &mut TuiState,
        event: Event,
        tx: mpsc::UnboundedSender<TuiEvent>,
        running_task: &mut Option<JoinHandle<()>>,
    ) -> Result<bool> {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(true);
                }

                if state.popup.is_some() {
                    return self.handle_popup_key(state, key.code);
                }

                match key.code {
                    KeyCode::Enter => {
                        if self.submit_input(state, tx, running_task).await? {
                            return Ok(true);
                        }
                    }
                    KeyCode::Backspace => {
                        state.input.pop();
                    }
                    KeyCode::Right | KeyCode::Tab => {
                        if let Some(hint) = matching_command_hints(&state.input).first() {
                            state.input = hint.command.to_string();
                        }
                    }
                    KeyCode::Esc => {
                        if state.loading {
                            if let Some(task) = running_task.take() {
                                task.abort();
                            }
                            state.loading = false;
                            state.request_started_at = None;
                            state
                                .messages
                                .push(MessageItem::Log(format!("[{}] Stopped", time_now())));
                        }
                    }
                    KeyCode::Char(ch) => {
                        state.input.push(ch);
                    }
                    _ => {}
                }
            }
            Event::Paste(text) => {
                state.input.push_str(&text);
            }
            _ => {}
        }

        Ok(false)
    }

    fn handle_popup_key(&self, state: &mut TuiState, code: KeyCode) -> Result<bool> {
        match code {
            KeyCode::Esc => {
                state.popup = None;
            }
            KeyCode::Up => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.select_prev();
                }
            }
            KeyCode::Down => {
                let len = filtered_popup_len(state);
                if let Some(popup) = state.popup.as_mut() {
                    popup.select_next(len);
                }
            }
            KeyCode::Backspace => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.pop_filter();
                    popup.reset_selection();
                }
                clamp_popup_selection(state);
            }
            KeyCode::Char(ch) => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.push_filter(ch);
                    popup.reset_selection();
                }
                clamp_popup_selection(state);
            }
            KeyCode::Left => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.select_prev_scope();
                    popup.reset_selection();
                }
                clamp_popup_selection(state);
            }
            KeyCode::Right | KeyCode::Tab => {
                if let Some(popup) = state.popup.as_mut() {
                    popup.select_next_scope();
                    popup.reset_selection();
                }
                clamp_popup_selection(state);
            }
            KeyCode::Enter => {
                let popup = state.popup.take();
                match popup {
                    Some(PopupState::Model {
                        options,
                        selected,
                        filter,
                    }) => {
                        let filtered = filtered_model_indices(&options, &filter);
                        if let Some(index) = filtered.get(selected) {
                            if let Some(model) = options.get(*index) {
                                state.model = model.clone();
                                state.messages.push(MessageItem::Log(format!(
                                    "[{}] Model -> {}",
                                    time_now(),
                                    model.label()
                                )));
                            }
                        }
                    }
                    Some(PopupState::Session {
                        options,
                        selected,
                        filter,
                        scope,
                    }) => {
                        let filtered =
                            filtered_session_indices(&options, &filter, &state.cwd, scope);
                        if let Some(index) = filtered.get(selected) {
                            if let Some(session) = options.get(*index) {
                                state.session_id = Some(session.session_id.clone());
                                state.session_started_at = Instant::now();
                                state.messages.clear();
                                state.messages.push(MessageItem::Log(format!(
                                    "[{}] Resumed {}",
                                    time_now(),
                                    session.session_id
                                )));
                            }
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
        Ok(false)
    }

    async fn submit_input(
        &self,
        state: &mut TuiState,
        tx: mpsc::UnboundedSender<TuiEvent>,
        running_task: &mut Option<JoinHandle<()>>,
    ) -> Result<bool> {
        let input = state.input.trim().to_string();
        if input.is_empty() || state.loading {
            return Ok(false);
        }

        state.input.clear();

        if input.starts_with('/') {
            return self.handle_command(state, &input).await;
        }

        state.messages.push(MessageItem::User(input.clone()));
        state.loading = true;
        state.spinner_index = 0;
        state.request_started_at = Some(Instant::now());

        let mut request = Request::new(input);
        request.session_id = state.session_id.clone();
        request.max_turns = self.max_turns;
        request.append_system_prompt = self.append_system_prompt.clone();

        let llm = self.active_llm(&state.model);
        let storage = self.storage.clone();
        let sink = TuiSink::new(tx.clone());

        *running_task = Some(tokio::spawn(async move {
            let result = RequestExecutor::open(request, llm, sink, storage)
                .execute()
                .await;
            let _ = tx.send(TuiEvent::RequestFinished(result));
        }));

        Ok(false)
    }

    async fn handle_command(&self, state: &mut TuiState, input: &str) -> Result<bool> {
        match input {
            "/help" => {
                state.messages.push(MessageItem::Log(format!(
                    "[{}] Commands: /new /resume /model /clear /exit",
                    time_now()
                )));
            }
            "/new" => {
                state.loading = false;
                state.request_started_at = None;
                state.session_id = None;
                state.session_started_at = Instant::now();
                state.messages.clear();
                state.messages.push(MessageItem::Log(format!(
                    "[{}] Initialized new session",
                    time_now()
                )));
            }
            "/resume" | "/sessions" => {
                let sessions = self
                    .storage
                    .list_sessions(ListSessions { limit: 20 })
                    .await?;
                if sessions.is_empty() {
                    state
                        .messages
                        .push(MessageItem::Error("no session available".into()));
                } else {
                    state.popup = Some(PopupState::Session {
                        options: sessions,
                        selected: 0,
                        filter: String::new(),
                        scope: SessionScope::CurrentFolder,
                    });
                }
            }
            "/model" => {
                let options = self.model_options();
                if options.is_empty() {
                    state
                        .messages
                        .push(MessageItem::Error("no configured model available".into()));
                } else {
                    let selected = options
                        .iter()
                        .position(|option| {
                            option.provider == state.model.provider
                                && option.model == state.model.model
                        })
                        .unwrap_or(0);
                    state.popup = Some(PopupState::Model {
                        options,
                        selected,
                        filter: String::new(),
                    });
                }
            }
            "/clear" => {
                state.messages.clear();
            }
            "/exit" | "/quit" => {
                return Ok(true);
            }
            _ => {
                state
                    .messages
                    .push(MessageItem::Error(format!("unknown command: {input}")));
            }
        }

        Ok(false)
    }

    fn handle_tui_event(&self, state: &mut TuiState, event: TuiEvent) {
        match event {
            TuiEvent::RunEvent(event) => {
                state.session_id = Some(event.session_id.clone());

                match event.kind {
                    RunEventKind::RunStarted => {
                        state.loading = true;
                        if state.request_started_at.is_none() {
                            state.request_started_at = Some(Instant::now());
                        }
                        state.messages.push(MessageItem::Log(format!(
                            "[{}] Run {}",
                            time_now(),
                            short_id(&event.run_id)
                        )));
                    }
                    RunEventKind::AssistantMessage => {
                        if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                            for block in payload.content {
                                match block {
                                    AssistantBlock::Text { text } => {
                                        if !text.trim().is_empty() {
                                            state.messages.push(MessageItem::Assistant(text));
                                        }
                                    }
                                    AssistantBlock::ToolUse { name, input, .. } => {
                                        let (title, lines) = tool_call_message(&name, &input);
                                        state.messages.push(MessageItem::ToolCall { title, lines });
                                    }
                                    AssistantBlock::Thinking { .. } => {}
                                }
                            }
                        }
                    }
                    RunEventKind::ToolResult => {
                        if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                            state.messages.push(MessageItem::ToolResult {
                                title: if payload.is_error {
                                    format!("{} failed", payload.tool_name)
                                } else {
                                    format!("{} completed", payload.tool_name)
                                },
                                lines: vec![if payload.content.trim().is_empty() {
                                    if payload.is_error {
                                        "Result: tool returned an error".into()
                                    } else {
                                        "Result: completed".into()
                                    }
                                } else {
                                    format!("Result: {}", summarize_text(&payload.content, 160))
                                }],
                                ok: !payload.is_error,
                            });
                        }
                    }
                    RunEventKind::Status | RunEventKind::Progress | RunEventKind::System => {
                        if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                            state.messages.push(MessageItem::Log(payload.message));
                        }
                    }
                    RunEventKind::CompactBoundary => {
                        if let Some(summary) = event
                            .payload
                            .get("summary")
                            .and_then(|value| value.as_str())
                        {
                            state.messages.push(MessageItem::Log(format!(
                                "Compacted: {}",
                                summarize_text(summary, 120)
                            )));
                        }
                    }
                    RunEventKind::Error => {
                        state.loading = false;
                        state.request_started_at = None;
                        if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                            state.messages.push(MessageItem::Error(payload.message));
                        }
                    }
                    RunEventKind::RunFinished => {
                        state.loading = false;
                        state.request_started_at = None;
                        if let Some(payload) = payload_as::<RequestFinishedPayload>(&event.payload)
                        {
                            state.messages.push(MessageItem::Log(format!(
                                "[{}] Completed in {} ms",
                                time_now(),
                                payload.duration_ms
                            )));
                        }
                    }
                    RunEventKind::PartialMessage
                    | RunEventKind::TaskNotification
                    | RunEventKind::RateLimit => {}
                }
            }
            TuiEvent::RequestFinished(result) => {
                state.loading = false;
                state.request_started_at = None;
                match result {
                    Ok(result) => {
                        state.session_id = Some(result.session_id);
                    }
                    Err(error) => {
                        state.messages.push(MessageItem::Error(error.to_string()));
                    }
                }
            }
        }
    }

    fn active_llm(&self, model: &ModelOption) -> LlmConfig {
        let config = self.config.provider_config(&model.provider);
        LlmConfig {
            provider: model.provider.clone(),
            api_key: config.api_key.clone(),
            base_url: config.base_url.clone(),
            model: model.model.clone(),
        }
    }

    fn model_options(&self) -> Vec<ModelOption> {
        let mut options = Vec::new();

        if !self.config.anthropic.api_key.is_empty() {
            options.push(ModelOption {
                provider: ProviderKind::Anthropic,
                model: self.config.anthropic.model.clone(),
            });
        }

        if !self.config.openai.api_key.is_empty() {
            options.push(ModelOption {
                provider: ProviderKind::OpenAi,
                model: self.config.openai.model.clone(),
            });
        }

        options
    }
}

fn enter_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode().map_err(|e| BendclawError::Cli(format!("failed to enable raw mode: {e}")))?;
    let backend = CrosstermBackend::new(stdout());
    Terminal::with_options(backend, TerminalOptions {
        viewport: Viewport::Inline(22),
    })
    .map_err(|e| BendclawError::Cli(format!("failed to create terminal: {e}")))
}

fn leave_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()
        .map_err(|e| BendclawError::Cli(format!("failed to disable raw mode: {e}")))?;
    terminal
        .show_cursor()
        .map_err(|e| BendclawError::Cli(format!("failed to show cursor: {e}")))
}

fn summarize_json(value: &serde_json::Value) -> String {
    if let Some(object) = value.as_object() {
        for key in [
            "command",
            "file_path",
            "path",
            "pattern",
            "query",
            "url",
            "name",
        ] {
            if let Some(value) = object.get(key) {
                if let Some(text) = value.as_str() {
                    return summarize_text(text, 120);
                }
            }
        }
    }

    summarize_text(&value.to_string(), 120)
}

fn summarize_text(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_string()
    } else {
        format!("{}...", value.chars().take(max).collect::<String>())
    }
}

fn tool_call_message(name: &str, input: &serde_json::Value) -> (String, Vec<String>) {
    let lowercase = name.to_lowercase();
    if lowercase.contains("grep") {
        return ("Grep 1 search".into(), vec![format!(
            "\"{}\"",
            summarize_json(input)
        )]);
    }
    if lowercase.contains("glob") {
        return ("Glob 1 pattern".into(), vec![summarize_json(input)]);
    }
    if lowercase.contains("read") {
        return ("Read 1 file".into(), vec![summarize_json(input)]);
    }

    (format!("{} call", name), vec![summarize_json(input)])
}

fn filtered_model_indices(options: &[ModelOption], filter: &str) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, option)| {
            filter.is_empty()
                || option.label().to_lowercase().contains(&filter)
                || option.model.to_lowercase().contains(&filter)
        })
        .map(|(index, _)| index)
        .collect()
}

fn filtered_session_indices(
    options: &[SessionMeta],
    filter: &str,
    cwd: &str,
    scope: SessionScope,
) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, session)| scope == SessionScope::All || session.cwd == cwd)
        .filter(|(_, session)| {
            if filter.is_empty() {
                return true;
            }

            session.session_id.to_lowercase().contains(&filter)
                || session.model.to_lowercase().contains(&filter)
                || session
                    .title
                    .as_ref()
                    .map(|title: &String| title.to_lowercase().contains(&filter))
                    .unwrap_or(false)
        })
        .map(|(index, _)| index)
        .collect()
}

fn filtered_popup_len(state: &TuiState) -> usize {
    match &state.popup {
        Some(PopupState::Model {
            options, filter, ..
        }) => filtered_model_indices(options, filter).len(),
        Some(PopupState::Session {
            options,
            filter,
            scope,
            ..
        }) => filtered_session_indices(options, filter, &state.cwd, *scope).len(),
        None => 0,
    }
}

fn clamp_popup_selection(state: &mut TuiState) {
    let len = filtered_popup_len(state);
    if len == 0 {
        if let Some(PopupState::Model { selected, .. }) = state.popup.as_mut() {
            *selected = 0;
        }
        if let Some(PopupState::Session { selected, .. }) = state.popup.as_mut() {
            *selected = 0;
        }
        return;
    }

    match state.popup.as_mut() {
        Some(PopupState::Model { selected, .. }) => {
            if *selected >= len {
                *selected = len - 1;
            }
        }
        Some(PopupState::Session { selected, .. }) => {
            if *selected >= len {
                *selected = len - 1;
            }
        }
        None => {}
    }
}

fn short_id(value: &str) -> String {
    value.chars().take(12).collect()
}

fn time_now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}
