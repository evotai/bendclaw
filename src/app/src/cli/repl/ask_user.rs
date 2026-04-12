use std::io::Write;

use bend_engine::tools::AskUserAnswer;
use bend_engine::tools::AskUserQuestion;
use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;

use super::markdown::ansi::display_width;
use super::render::with_terminal;
use super::render::DIM;
use super::render::GREEN;
use super::render::RESET;
use super::render::YELLOW;

const ERASE_LINE: &str = "\x1b[K";
const CYAN: &str = "\x1b[36m";
const BOLD: &str = "\x1b[1m";

fn cursor_up(n: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("\x1b[{n}A")
    }
}

fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
        .max(1)
}

pub fn physical_row_count(line: &str, term_width: usize) -> usize {
    let w = display_width(line);
    if w == 0 {
        return 1;
    }
    let tw = term_width.max(1);
    w.div_ceil(tw)
}

// ---------------------------------------------------------------------------
// State machine — pure logic, no IO
// ---------------------------------------------------------------------------

/// Per-question UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionState {
    /// Currently highlighted option index (0..=options.len() where last = none).
    pub selected: usize,
    /// Confirmed answer, if any.
    pub answer: Option<String>,
    /// Draft text for custom input (preserved across mode switches).
    pub draft: String,
}

/// Whether the user is selecting an option or typing custom text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Selecting,
    Typing,
}

/// Full UI state for the ask_user interaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AskUserState {
    pub active_question: usize,
    pub input_mode: InputMode,
    pub states: Vec<QuestionState>,
}

impl AskUserState {
    pub fn new(question_count: usize) -> Self {
        Self {
            active_question: 0,
            input_mode: InputMode::Selecting,
            states: vec![
                QuestionState {
                    selected: 0,
                    answer: None,
                    draft: String::new(),
                };
                question_count
            ],
        }
    }
}

/// Action returned by the state machine after processing a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskUserAction {
    /// Re-render the UI with the updated state.
    Redraw,
    /// Submit all answers and exit.
    Submit(AskUserResponse),
    /// User pressed Ctrl-C — abort the run.
    ExitRun,
    /// User pressed Esc in selection mode — cancel the current turn.
    CancelTurn,
    /// Key was ignored, no state change.
    Noop,
}

/// Total visible items for a question (options + "None of the above").
pub fn total_items_for(question: &AskUserQuestion) -> usize {
    question.options.len() + 1
}

/// The index of the "None of the above" item for a question.
pub fn none_index_for(question: &AskUserQuestion) -> usize {
    question.options.len()
}

/// Process a single key press and return the next state + action.
///
/// This is a pure function — no IO, no terminal access.
pub fn handle_key(
    request: &AskUserRequest,
    mut state: AskUserState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserState, AskUserAction) {
    let q = &request.questions[state.active_question];

    match state.input_mode {
        InputMode::Selecting => handle_selecting(request, state, q, code, modifiers),
        InputMode::Typing => handle_typing(request, &mut state, code, modifiers),
    }
}

fn handle_selecting(
    request: &AskUserRequest,
    mut state: AskUserState,
    q: &AskUserQuestion,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserState, AskUserAction) {
    let total = total_items_for(q);
    let none_idx = none_index_for(q);
    let qs = &mut state.states[state.active_question];
    let selected = qs.selected;

    match code {
        // -- vertical navigation --
        KeyCode::Up | KeyCode::Char('k') => {
            qs.selected = if selected > 0 {
                selected - 1
            } else {
                total - 1
            };
            if qs.selected == none_idx {
                state.input_mode = InputMode::Typing;
            }
            (state, AskUserAction::Redraw)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            qs.selected = (selected + 1) % total;
            if qs.selected == none_idx {
                state.input_mode = InputMode::Typing;
            }
            (state, AskUserAction::Redraw)
        }

        // -- horizontal navigation (switch question) --
        KeyCode::Left | KeyCode::Char('h') if request.questions.len() > 1 => {
            let n = request.questions.len();
            state.active_question = if state.active_question > 0 {
                state.active_question - 1
            } else {
                n - 1
            };
            (state, AskUserAction::Redraw)
        }
        KeyCode::Right | KeyCode::Char('l') if request.questions.len() > 1 => {
            state.active_question = (state.active_question + 1) % request.questions.len();
            (state, AskUserAction::Redraw)
        }

        // -- confirm selection --
        KeyCode::Enter => {
            if selected < q.options.len() {
                let label = q.options[selected].label.clone();
                qs.answer = Some(label);
                advance_or_submit(request, state)
            } else {
                state.input_mode = InputMode::Typing;
                (state, AskUserAction::Redraw)
            }
        }

        // -- digit shortcut --
        KeyCode::Char(ch @ '1'..='9') => {
            let idx = (ch as usize) - ('1' as usize);
            if idx < q.options.len() {
                let label = q.options[idx].label.clone();
                qs.answer = Some(label);
                advance_or_submit(request, state)
            } else if idx == none_idx {
                qs.selected = none_idx;
                state.input_mode = InputMode::Typing;
                (state, AskUserAction::Redraw)
            } else {
                (state, AskUserAction::Noop)
            }
        }

        // -- cancel / exit --
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            (state, AskUserAction::ExitRun)
        }
        KeyCode::Esc => (state, AskUserAction::CancelTurn),

        _ => (state, AskUserAction::Noop),
    }
}

fn handle_typing(
    request: &AskUserRequest,
    state: &mut AskUserState,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserState, AskUserAction) {
    let qs = &mut state.states[state.active_question];

    match code {
        KeyCode::Enter => {
            let trimmed = qs.draft.trim().to_string();
            if trimmed.is_empty() {
                state.input_mode = InputMode::Selecting;
                (state.clone(), AskUserAction::Redraw)
            } else {
                qs.answer = Some(trimmed);
                let s = state.clone();
                advance_or_submit(request, s)
            }
        }
        KeyCode::Esc => {
            state.input_mode = InputMode::Selecting;
            (state.clone(), AskUserAction::Redraw)
        }
        KeyCode::Backspace => {
            qs.draft.pop();
            (state.clone(), AskUserAction::Redraw)
        }

        // -- horizontal navigation in typing mode --
        KeyCode::Left if request.questions.len() > 1 => {
            let n = request.questions.len();
            state.active_question = if state.active_question > 0 {
                state.active_question - 1
            } else {
                n - 1
            };
            state.input_mode = InputMode::Selecting;
            (state.clone(), AskUserAction::Redraw)
        }
        KeyCode::Right if request.questions.len() > 1 => {
            state.active_question = (state.active_question + 1) % request.questions.len();
            state.input_mode = InputMode::Selecting;
            (state.clone(), AskUserAction::Redraw)
        }

        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            (state.clone(), AskUserAction::ExitRun)
        }
        KeyCode::Char(ch) => {
            qs.draft.push(ch);
            (state.clone(), AskUserAction::Redraw)
        }
        _ => (state.clone(), AskUserAction::Noop),
    }
}

/// After confirming an answer, jump to the next unanswered question.
/// If all answered, submit.
fn advance_or_submit(
    request: &AskUserRequest,
    mut state: AskUserState,
) -> (AskUserState, AskUserAction) {
    let n = request.questions.len();

    for offset in 1..=n {
        let idx = (state.active_question + offset) % n;
        if state.states[idx].answer.is_none() {
            state.active_question = idx;
            state.input_mode = InputMode::Selecting;
            return (state, AskUserAction::Redraw);
        }
    }

    let answers = request
        .questions
        .iter()
        .zip(state.states.iter())
        .map(|(q, qs)| AskUserAnswer {
            header: q.header.clone(),
            question: q.question.clone(),
            answer: qs.answer.clone().unwrap_or_default(),
        })
        .collect();

    (
        state,
        AskUserAction::Submit(AskUserResponse::Answered(answers)),
    )
}

// ---------------------------------------------------------------------------
// Rendering — pure string building, no IO
// ---------------------------------------------------------------------------

pub fn build_question_block(
    request: &AskUserRequest,
    state: &AskUserState,
    term_width: usize,
) -> (String, usize) {
    let mut out = String::new();
    let mut rows: usize = 0;

    let q = &request.questions[state.active_question];
    let qs = &state.states[state.active_question];
    let typing = match state.input_mode {
        InputMode::Typing => Some(qs.draft.as_str()),
        InputMode::Selecting => None,
    };

    // Tab bar (only for multi-question)
    if request.questions.len() > 1 {
        let tab_line = build_tab_bar(request, state);
        let full_line = format!("\r{ERASE_LINE}  {tab_line}");
        out.push_str(&full_line);
        out.push_str("\r\n");
        rows += physical_row_count(&full_line, term_width);

        out.push_str(&format!("{ERASE_LINE}\r\n"));
        rows += 1;
    }

    // Question text
    let line = format!("\r{ERASE_LINE}  {CYAN}❓ {BOLD}{}{RESET}", q.question);
    out.push_str(&line);
    out.push_str("\r\n");
    rows += physical_row_count(&line, term_width);

    out.push_str(&format!("{ERASE_LINE}\r\n"));
    rows += 1;

    // Options
    for (i, opt) in q.options.iter().enumerate() {
        let num = i + 1;
        let is_selected = i == qs.selected;
        let is_answered = qs.answer.as_deref() == Some(&opt.label);
        let marker = if is_selected { "›" } else { " " };
        let check = if is_answered { " ✓" } else { "" };
        let highlight = if is_selected {
            YELLOW
        } else if is_answered {
            GREEN
        } else {
            DIM
        };

        let label_line = format!(
            "{ERASE_LINE}  {highlight}{marker} {num}. {}{check}{RESET}",
            opt.label
        );
        out.push_str(&label_line);
        out.push_str("\r\n");
        rows += physical_row_count(&label_line, term_width);

        let desc_line = format!("{ERASE_LINE}  {DIM}     {}{RESET}", opt.description);
        out.push_str(&desc_line);
        out.push_str("\r\n");
        rows += physical_row_count(&desc_line, term_width);
    }

    // None of the above / inline input
    let none_idx = none_index_for(q);
    let none_num = none_idx + 1;
    let is_none_selected = qs.selected == none_idx;
    let marker = if is_none_selected { "›" } else { " " };
    // Check if the current answer is a custom input (not matching any option label)
    let is_custom_answered = qs
        .answer
        .as_ref()
        .is_some_and(|a| !q.options.iter().any(|o| o.label == *a));

    if let Some(input) = typing {
        // Typing mode: cursor ready for input, no placeholder
        let none_line = if input.is_empty() {
            format!("{ERASE_LINE}  {YELLOW}{marker} {none_num}. {RESET}█",)
        } else {
            format!("{ERASE_LINE}  {YELLOW}{marker} {none_num}. {RESET}{input}█",)
        };
        out.push_str(&none_line);
        out.push_str("\r\n");
        rows += physical_row_count(&none_line, term_width);

        out.push_str(&format!("{ERASE_LINE}\r\n"));
        rows += 1;

        let hint_line = format!("{ERASE_LINE}  {DIM}[Enter submit  Esc back to list]{RESET}");
        out.push_str(&hint_line);
        out.push_str("\r\n");
        rows += physical_row_count(&hint_line, term_width);
    } else if is_none_selected {
        // Selecting mode, cursor on none: show placeholder with cursor
        let none_line = if is_custom_answered {
            // Show the custom answer
            let ans = qs.answer.as_deref().unwrap_or("");
            format!("{ERASE_LINE}  {YELLOW}{marker} {none_num}. {GREEN}{ans} ✓{RESET}█",)
        } else {
            format!("{ERASE_LINE}  {YELLOW}{marker} {none_num}. {DIM}Type something...{RESET}█",)
        };
        out.push_str(&none_line);
        out.push_str("\r\n");
        rows += physical_row_count(&none_line, term_width);

        out.push_str(&format!("{ERASE_LINE}\r\n"));
        rows += 1;

        let nav = if request.questions.len() > 1 {
            "←→ question  "
        } else {
            ""
        };
        let footer_line =
            format!("{ERASE_LINE}  {DIM}[{nav}↑↓ select  Enter edit  Esc cancel]{RESET}",);
        out.push_str(&footer_line);
        out.push_str("\r\n");
        rows += physical_row_count(&footer_line, term_width);
    } else {
        // Selecting mode, cursor NOT on none
        let highlight = if is_custom_answered { GREEN } else { DIM };
        let check = if is_custom_answered { " ✓" } else { "" };
        let label = if is_custom_answered {
            qs.answer.as_deref().unwrap_or("None of the above")
        } else {
            "None of the above (type your own)"
        };
        let none_line =
            format!("{ERASE_LINE}  {highlight}{marker} {none_num}. {label}{check}{RESET}",);
        out.push_str(&none_line);
        out.push_str("\r\n");
        rows += physical_row_count(&none_line, term_width);

        out.push_str(&format!("{ERASE_LINE}\r\n"));
        rows += 1;

        let nav = if request.questions.len() > 1 {
            "←→ question  "
        } else {
            ""
        };
        let footer_line = format!(
            "{ERASE_LINE}  {DIM}[{nav}↑↓ select  Enter confirm  1-{} pick  Esc cancel]{RESET}",
            none_num
        );
        out.push_str(&footer_line);
        out.push_str("\r\n");
        rows += physical_row_count(&footer_line, term_width);
    }

    (out, rows)
}

fn build_tab_bar(request: &AskUserRequest, state: &AskUserState) -> String {
    let mut tabs = Vec::new();
    for (i, q) in request.questions.iter().enumerate() {
        let qs = &state.states[i];
        let is_active = i == state.active_question;
        let is_answered = qs.answer.is_some();

        let checkbox = if is_answered { "☑" } else { "☐" };
        let tab = if is_active {
            format!("{YELLOW}{checkbox} {}‹{RESET}", q.header)
        } else if is_answered {
            format!("{GREEN}{checkbox} {}{RESET}", q.header)
        } else {
            format!("{DIM}{checkbox} {}{RESET}", q.header)
        };
        tabs.push(tab);
    }
    tabs.join("  ")
}

pub fn build_confirmation(label: &str) -> String {
    format!("  {GREEN}✓ {label}{RESET}")
}

pub fn build_skipped() -> String {
    format!("  {DIM}— skipped{RESET}")
}

// ---------------------------------------------------------------------------
// Terminal IO loop — thin wrapper around the state machine
// ---------------------------------------------------------------------------

pub enum AskUserUiResult {
    Answer(AskUserResponse),
    ExitRun,
    CancelTurn,
}

pub fn render_and_select(request: &AskUserRequest) -> std::io::Result<AskUserUiResult> {
    let mut state = AskUserState::new(request.questions.len());
    let mut prev_lines: usize = 0;
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            let term_width = terminal_width();
            let (output, line_count) = build_question_block(request, &state, term_width);
            with_terminal(|stdout| {
                if prev_lines > 0 {
                    let _ = write!(stdout, "\r{}", cursor_up(prev_lines));
                }
                let _ = write!(stdout, "{output}");
                let _ = stdout.flush();
            });
            prev_lines = line_count;
            needs_redraw = false;
        }

        if !poll(std::time::Duration::from_millis(100))? {
            continue;
        }

        match read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                let (next_state, action) = handle_key(request, state, key.code, key.modifiers);
                state = next_state;

                match action {
                    AskUserAction::Redraw => {
                        needs_redraw = true;
                    }
                    AskUserAction::Submit(response) => {
                        clear_block(prev_lines);
                        match &response {
                            AskUserResponse::Answered(answers) => {
                                for a in answers {
                                    print_result(&build_confirmation(&format!(
                                        "{}: {}",
                                        a.header, a.answer
                                    )));
                                }
                            }
                            AskUserResponse::Skipped => {
                                print_result(&build_skipped());
                            }
                        }
                        return Ok(AskUserUiResult::Answer(response));
                    }
                    AskUserAction::ExitRun => {
                        clear_block(prev_lines);
                        return Ok(AskUserUiResult::ExitRun);
                    }
                    AskUserAction::CancelTurn => {
                        clear_block(prev_lines);
                        print_result(&build_skipped());
                        return Ok(AskUserUiResult::CancelTurn);
                    }
                    AskUserAction::Noop => {}
                }
            }
            _ => {}
        }
    }
}

fn clear_block(line_count: usize) {
    if line_count == 0 {
        return;
    }
    with_terminal(|stdout| {
        let _ = write!(stdout, "\r{}", cursor_up(line_count));
        for _ in 0..line_count {
            let _ = write!(stdout, "{ERASE_LINE}\r\n");
        }
        let _ = write!(stdout, "\r{}", cursor_up(line_count));
        let _ = stdout.flush();
    });
}

fn print_result(text: &str) {
    with_terminal(|stdout| {
        let _ = write!(stdout, "{text}\r\n\r\n");
        let _ = stdout.flush();
    });
}
