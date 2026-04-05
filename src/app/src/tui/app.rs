use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use bend_agent::RunSummary;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::backend::CrosstermBackend;
use ratatui::text::Text;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
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
use crate::tui::state::ModelOption;
use crate::tui::state::PopupState;
use crate::tui::state::SessionScope;
use crate::tui::state::TranscriptBlock;
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

        let mut viewport_height = view::desired_inline_height(&state);
        let mut terminal = enter_terminal(viewport_height)?;
        flush_block(&mut terminal, view::welcome_block(&state))?;

        let result = loop {
            let desired_height = view::desired_inline_height(&state);
            if desired_height != viewport_height {
                terminal = reopen_terminal(desired_height)?;
                viewport_height = desired_height;
            }

            terminal
                .draw(|frame| view::render(frame, &state))
                .map_err(|e| BendclawError::Cli(format!("failed to draw tui: {e}")))?;

            while let Ok(event) = rx.try_recv() {
                let blocks = self.handle_tui_event(&mut state, event);
                flush_blocks(&mut terminal, blocks)?;
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
                    Ok(action) => {
                        flush_blocks(&mut terminal, action.blocks)?;
                        if action.exit {
                            break Ok(());
                        }
                    }
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
    ) -> Result<TerminalAction> {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    return Ok(TerminalAction::exit());
                }

                if state.popup.is_some() {
                    return self.handle_popup_key(state, key.code);
                }

                match key.code {
                    KeyCode::Enter => {
                        return self.submit_input(state, tx, running_task).await;
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
                            state.streaming_assistant.clear();
                            state.status_message = Some("Stopped".into());
                            return Ok(TerminalAction::with_block(view::log_block(format!(
                                "[{}] stopped",
                                time_now()
                            ))));
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
            Event::Resize(_, _) => {}
            _ => {}
        }

        Ok(TerminalAction::none())
    }

    fn handle_popup_key(&self, state: &mut TuiState, code: KeyCode) -> Result<TerminalAction> {
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
                                state.status_message = Some(format!("Model {}", model.label()));
                                return Ok(TerminalAction::with_block(view::log_block(format!(
                                    "[{}] model -> {}",
                                    time_now(),
                                    model.label()
                                ))));
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
                                state.status_message =
                                    Some(format!("Resumed {}", short_id(&session.session_id)));
                                return Ok(TerminalAction::with_block(view::log_block(format!(
                                    "[{}] resumed {}",
                                    time_now(),
                                    summarize_session_title(session)
                                ))));
                            }
                        }
                    }
                    None => {}
                }
            }
            _ => {}
        }
        Ok(TerminalAction::none())
    }

    async fn submit_input(
        &self,
        state: &mut TuiState,
        tx: mpsc::UnboundedSender<TuiEvent>,
        running_task: &mut Option<JoinHandle<()>>,
    ) -> Result<TerminalAction> {
        let input = state.input.trim().to_string();
        if input.is_empty() || state.loading {
            return Ok(TerminalAction::none());
        }

        state.input.clear();

        if input.starts_with('/') {
            return self.handle_command(state, &input).await;
        }

        state.loading = true;
        state.spinner_index = 0;
        state.request_started_at = Some(Instant::now());
        state.status_message = Some("Streaming...".into());
        state.streaming_assistant.clear();

        let mut request = Request::new(input.clone());
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

        Ok(TerminalAction::with_block(view::user_block(&input)))
    }

    async fn handle_command(&self, state: &mut TuiState, input: &str) -> Result<TerminalAction> {
        match input {
            "/help" => {
                return Ok(TerminalAction::with_block(view::log_block(
                    "Commands: /new /sessions /model /clear /exit",
                )));
            }
            "/new" => {
                state.loading = false;
                state.request_started_at = None;
                state.streaming_assistant.clear();
                state.session_id = None;
                state.session_started_at = Instant::now();
                state.status_message = Some("New session".into());
                return Ok(TerminalAction::with_block(view::log_block(format!(
                    "[{}] initialized new session",
                    time_now()
                ))));
            }
            "/resume" | "/sessions" => {
                let sessions = self
                    .storage
                    .list_sessions(ListSessions { limit: 20 })
                    .await?;
                if sessions.is_empty() {
                    return Ok(TerminalAction::with_block(view::error_block(
                        "no session available",
                    )));
                }

                state.popup = Some(PopupState::Session {
                    options: sessions,
                    selected: 0,
                    filter: String::new(),
                    scope: SessionScope::CurrentFolder,
                });
            }
            "/model" => {
                let options = self.model_options();
                if options.is_empty() {
                    return Ok(TerminalAction::with_block(view::error_block(
                        "no configured model available",
                    )));
                }

                let selected = options
                    .iter()
                    .position(|option| {
                        option.provider == state.model.provider && option.model == state.model.model
                    })
                    .unwrap_or(0);

                state.popup = Some(PopupState::Model {
                    options,
                    selected,
                    filter: String::new(),
                });
            }
            "/clear" => {
                return Ok(TerminalAction::with_block(view::divider_block()));
            }
            "/exit" | "/quit" => {
                return Ok(TerminalAction::exit());
            }
            _ => {
                return Ok(TerminalAction::with_block(view::error_block(format!(
                    "unknown command: {input}"
                ))));
            }
        }

        Ok(TerminalAction::none())
    }

    fn handle_tui_event(&self, state: &mut TuiState, event: TuiEvent) -> Vec<TranscriptBlock> {
        let mut blocks = Vec::new();
        match event {
            TuiEvent::RunEvent(event) => {
                state.session_id = Some(event.session_id.clone());

                match event.kind {
                    RunEventKind::RunStarted => {
                        state.loading = true;
                        if state.request_started_at.is_none() {
                            state.request_started_at = Some(Instant::now());
                        }
                        state.status_message = Some(format!("Run {}", short_id(&event.run_id)));
                    }
                    RunEventKind::AssistantMessage => {
                        if let Some(payload) = payload_as::<AssistantPayload>(&event.payload) {
                            for block in payload.content {
                                match block {
                                    AssistantBlock::Text { text } => {
                                        let rendered = if state.streaming_assistant.is_empty() {
                                            text
                                        } else {
                                            std::mem::take(&mut state.streaming_assistant)
                                        };
                                        if !rendered.trim().is_empty() {
                                            blocks.push(view::assistant_block(&rendered));
                                        }
                                    }
                                    AssistantBlock::ToolUse { name, input, .. } => {
                                        let (title, lines) = tool_call_message(&name, &input);
                                        blocks.push(view::tool_call_block(&title, &lines));
                                    }
                                    AssistantBlock::Thinking { .. } => {}
                                }
                            }
                        }
                    }
                    RunEventKind::ToolResult => {
                        if let Some(payload) = payload_as::<ToolResultPayload>(&event.payload) {
                            blocks.push(view::tool_result_block(
                                &tool_result_title(&payload),
                                &[tool_result_line(&payload)],
                                !payload.is_error,
                            ));
                        }
                    }
                    RunEventKind::Status | RunEventKind::Progress => {
                        if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                            if !payload.message.trim().is_empty() {
                                state.status_message = Some(payload.message);
                            }
                        }
                    }
                    RunEventKind::System => {}
                    RunEventKind::CompactBoundary => {
                        if let Some(summary) = event
                            .payload
                            .get("summary")
                            .and_then(|value| value.as_str())
                        {
                            blocks.push(view::log_block(format!(
                                "Compacted: {}",
                                summarize_text(summary, 120)
                            )));
                        }
                    }
                    RunEventKind::Error => {
                        state.loading = false;
                        state.request_started_at = None;
                        state.streaming_assistant.clear();
                        if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                            state.status_message = Some("Error".into());
                            blocks.push(view::error_block(payload.message));
                        }
                    }
                    RunEventKind::RunFinished => {
                        state.loading = false;
                        state.request_started_at = None;
                        state.streaming_assistant.clear();
                        if let Some(payload) = payload_as::<RequestFinishedPayload>(&event.payload)
                        {
                            state.status_message =
                                Some(format!("Completed in {} ms", payload.duration_ms));
                            blocks.push(view::summary_block(
                                &build_run_summary_badge(&payload),
                                &build_run_summary_detail(&payload),
                            ));
                        }
                    }
                    RunEventKind::PartialMessage => {
                        if let Some(payload) = payload_as::<MessagePayload>(&event.payload) {
                            state.streaming_assistant.push_str(&payload.message);
                            state.status_message = Some("Streaming...".into());
                        }
                    }
                    RunEventKind::TaskNotification => {
                        if let Some(message) = event
                            .payload
                            .get("message")
                            .and_then(|value| value.as_str())
                        {
                            state.status_message = Some(message.to_string());
                        }
                    }
                    RunEventKind::RateLimit => {
                        if let Some(message) = event
                            .payload
                            .get("message")
                            .and_then(|value| value.as_str())
                        {
                            blocks.push(view::error_block(message.to_string()));
                        }
                    }
                }
            }
            TuiEvent::RequestFinished(result) => {
                state.loading = false;
                state.request_started_at = None;
                state.streaming_assistant.clear();
                match result {
                    Ok(result) => {
                        state.session_id = Some(result.session_id);
                    }
                    Err(error) => {
                        state.status_message = Some("Error".into());
                        blocks.push(view::error_block(error.to_string()));
                    }
                }
            }
        }
        blocks
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

struct TerminalAction {
    exit: bool,
    blocks: Vec<TranscriptBlock>,
}

impl TerminalAction {
    fn none() -> Self {
        Self {
            exit: false,
            blocks: Vec::new(),
        }
    }

    fn exit() -> Self {
        Self {
            exit: true,
            blocks: Vec::new(),
        }
    }

    fn with_block(block: TranscriptBlock) -> Self {
        Self {
            exit: false,
            blocks: vec![block],
        }
    }
}

fn enter_terminal(height: u16) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode().map_err(|e| BendclawError::Cli(format!("failed to enable raw mode: {e}")))?;
    create_terminal(height)
}

fn reopen_terminal(height: u16) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    create_terminal(height)
}

fn create_terminal(height: u16) -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    let backend = CrosstermBackend::new(stdout());
    Terminal::with_options(backend, TerminalOptions {
        viewport: Viewport::Inline(height),
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

fn flush_blocks(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    blocks: Vec<TranscriptBlock>,
) -> Result<()> {
    for block in blocks {
        flush_block(terminal, block)?;
    }
    Ok(())
}

fn flush_block(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    block: TranscriptBlock,
) -> Result<()> {
    if block.lines.is_empty() {
        return Ok(());
    }

    let width = terminal
        .size()
        .map_err(|e| BendclawError::Cli(format!("failed to read terminal size: {e}")))?
        .width
        .max(1) as usize;
    let height = block
        .lines
        .iter()
        .map(|line| line.width().max(1).div_ceil(width))
        .sum::<usize>()
        .max(1) as u16;
    let text = Text::from(block.lines);
    terminal
        .insert_before(height, move |buf| {
            Paragraph::new(text.clone())
                .wrap(Wrap { trim: false })
                .render(buf.area, buf);
        })
        .map_err(|e| BendclawError::Cli(format!("failed to write transcript: {e}")))?;
    Ok(())
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

    (format!("{name} call"), vec![summarize_json(input)])
}

fn tool_result_title(payload: &ToolResultPayload) -> String {
    if payload.is_error {
        format!("{} failed", payload.tool_name)
    } else {
        format!("{} completed", payload.tool_name)
    }
}

fn tool_result_line(payload: &ToolResultPayload) -> String {
    if payload.content.trim().is_empty() {
        if payload.is_error {
            "Result: tool returned an error".into()
        } else {
            "Result: completed".into()
        }
    } else {
        format!("Result: {}", summarize_text(&payload.content, 160))
    }
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
                    .map(|title| title.to_lowercase().contains(&filter))
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
        Some(PopupState::Model { selected, .. }) | Some(PopupState::Session { selected, .. }) => {
            if *selected >= len {
                *selected = len - 1;
            }
        }
        None => {}
    }
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn summarize_session_title(session: &SessionMeta) -> String {
    session
        .title
        .clone()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| short_id(&session.session_id))
}

fn time_now() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

fn build_run_summary_badge(payload: &RequestFinishedPayload) -> String {
    let summary = serde_json::from_value::<RunSummary>(payload.summary.clone()).unwrap_or_default();
    let mut parts = vec![
        format!("RUN {}", time_now()),
        human_duration(payload.duration_ms),
    ];

    if let Some(ttfb_ms) = summary.stream.first_ttfb_ms {
        if ttfb_ms > 0 {
            parts.push(format!("ttfb {}", human_duration(ttfb_ms)));
        }
    }
    if let Some(ttft_ms) = summary.stream.first_ttft_ms {
        if ttft_ms > 0 {
            parts.push(format!("ttft {}", human_duration(ttft_ms)));
        }
    }

    parts.join(" · ")
}

fn build_run_summary_detail(payload: &RequestFinishedPayload) -> String {
    let summary = serde_json::from_value::<RunSummary>(payload.summary.clone()).unwrap_or_default();
    format!(
        "turns {}  |  tokens {}  |  llm {}  |  tools {}",
        payload.num_turns,
        payload
            .usage
            .get("input_tokens")
            .and_then(|value| value.as_u64())
            .unwrap_or_default()
            + payload
                .usage
                .get("output_tokens")
                .and_then(|value| value.as_u64())
                .unwrap_or_default(),
        human_duration(summary.api_duration_ms),
        human_duration(summary.tool_duration_ms)
    )
}

fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}
