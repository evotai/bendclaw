use std::io::Write;

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

/// UI mode for the ask_user selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskUserMode {
    /// Arrow-key / digit selection mode.
    Selecting { selected: usize },
    /// Free-text input mode.
    Typing { selected: usize, input: String },
}

/// Action returned by the state machine after processing a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskUserAction {
    /// Re-render the UI with the updated mode.
    Redraw,
    /// Submit a final answer and exit.
    Submit(AskUserResponse),
    /// User pressed Ctrl-C — abort the run.
    ExitRun,
    /// User pressed Esc in selection mode — cancel the current turn.
    CancelTurn,
    /// Key was ignored, no state change.
    Noop,
}

/// Total visible items (options + "None of the above").
pub fn total_items(request: &AskUserRequest) -> usize {
    request.options.len() + 1
}

/// The display number for the "None of the above" item.
pub fn none_number(request: &AskUserRequest) -> usize {
    request.options.len() + 1
}

/// Process a single key press and return the next mode + action.
///
/// This is a pure function — no IO, no terminal access.
pub fn handle_key(
    request: &AskUserRequest,
    mode: AskUserMode,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserMode, AskUserAction) {
    let total = total_items(request);

    match mode {
        AskUserMode::Selecting { selected } => {
            handle_selecting(request, selected, total, code, modifiers)
        }
        AskUserMode::Typing {
            selected,
            mut input,
        } => handle_typing(selected, &mut input, code, modifiers),
    }
}

fn handle_selecting(
    request: &AskUserRequest,
    selected: usize,
    total: usize,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserMode, AskUserAction) {
    match code {
        KeyCode::Up | KeyCode::Char('k') => {
            let next = if selected > 0 {
                selected - 1
            } else {
                total - 1
            };
            (
                AskUserMode::Selecting { selected: next },
                AskUserAction::Redraw,
            )
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let next = (selected + 1) % total;
            (
                AskUserMode::Selecting { selected: next },
                AskUserAction::Redraw,
            )
        }

        KeyCode::Enter => {
            if selected < request.options.len() {
                let label = request.options[selected].label.clone();
                (
                    AskUserMode::Selecting { selected },
                    AskUserAction::Submit(AskUserResponse::Selected(label)),
                )
            } else {
                (
                    AskUserMode::Typing {
                        selected,
                        input: String::new(),
                    },
                    AskUserAction::Redraw,
                )
            }
        }

        KeyCode::Char(ch @ '1'..='9') => {
            let idx = (ch as usize) - ('1' as usize);
            if idx < request.options.len() {
                let label = request.options[idx].label.clone();
                (
                    AskUserMode::Selecting { selected },
                    AskUserAction::Submit(AskUserResponse::Selected(label)),
                )
            } else if idx == request.options.len() {
                (
                    AskUserMode::Typing {
                        selected: idx,
                        input: String::new(),
                    },
                    AskUserAction::Redraw,
                )
            } else {
                (AskUserMode::Selecting { selected }, AskUserAction::Noop)
            }
        }

        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
            (AskUserMode::Selecting { selected }, AskUserAction::ExitRun)
        }

        KeyCode::Esc => (
            AskUserMode::Selecting { selected },
            AskUserAction::CancelTurn,
        ),

        _ => (AskUserMode::Selecting { selected }, AskUserAction::Noop),
    }
}

fn handle_typing(
    selected: usize,
    input: &mut String,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> (AskUserMode, AskUserAction) {
    match code {
        KeyCode::Enter => {
            let trimmed = input.trim().to_string();
            if trimmed.is_empty() {
                // Empty input — go back to selection
                (AskUserMode::Selecting { selected }, AskUserAction::Redraw)
            } else {
                (
                    AskUserMode::Typing {
                        selected,
                        input: input.clone(),
                    },
                    AskUserAction::Submit(AskUserResponse::Custom(trimmed)),
                )
            }
        }
        KeyCode::Esc => {
            // Back to selection mode
            (AskUserMode::Selecting { selected }, AskUserAction::Redraw)
        }
        KeyCode::Backspace => {
            input.pop();
            (
                AskUserMode::Typing {
                    selected,
                    input: input.clone(),
                },
                AskUserAction::Redraw,
            )
        }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => (
            AskUserMode::Typing {
                selected,
                input: input.clone(),
            },
            AskUserAction::ExitRun,
        ),
        KeyCode::Char(ch) => {
            input.push(ch);
            (
                AskUserMode::Typing {
                    selected,
                    input: input.clone(),
                },
                AskUserAction::Redraw,
            )
        }
        _ => (
            AskUserMode::Typing {
                selected,
                input: input.clone(),
            },
            AskUserAction::Noop,
        ),
    }
}

// ---------------------------------------------------------------------------
// Rendering — pure string building, no IO
// ---------------------------------------------------------------------------

pub fn build_question_block(
    request: &AskUserRequest,
    selected: usize,
    term_width: usize,
) -> (String, usize) {
    build_question_block_inner(request, selected, term_width, None)
}

pub fn build_question_block_typing(
    request: &AskUserRequest,
    selected: usize,
    term_width: usize,
    input: &str,
) -> (String, usize) {
    build_question_block_inner(request, selected, term_width, Some(input))
}

fn build_question_block_inner(
    request: &AskUserRequest,
    selected: usize,
    term_width: usize,
    typing: Option<&str>,
) -> (String, usize) {
    let mut out = String::new();
    let mut rows: usize = 0;

    let line = format!("\r{ERASE_LINE}  {CYAN}❓ {BOLD}{}{RESET}", request.question);
    out.push_str(&line);
    out.push_str("\r\n");
    rows += physical_row_count(&line, term_width);

    out.push_str(&format!("{ERASE_LINE}\r\n"));
    rows += 1;

    for (i, opt) in request.options.iter().enumerate() {
        let num = i + 1;
        let is_selected = i == selected;
        let marker = if is_selected { "›" } else { " " };
        let highlight = if is_selected { YELLOW } else { DIM };

        let label_line = format!(
            "{ERASE_LINE}  {highlight}{marker} {num}. {}{RESET}",
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

    let none_idx = request.options.len();
    let none_num = none_idx + 1;
    let is_none_selected = selected == none_idx;
    let marker = if is_none_selected { "›" } else { " " };
    let highlight = if is_none_selected { YELLOW } else { DIM };
    let none_line = format!(
        "{ERASE_LINE}  {highlight}{marker} {none_num}. None of the above (type your own){RESET}",
    );
    out.push_str(&none_line);
    out.push_str("\r\n");
    rows += physical_row_count(&none_line, term_width);

    out.push_str(&format!("{ERASE_LINE}\r\n"));
    rows += 1;

    if let Some(input) = typing {
        // Inline input mode: show input field instead of footer
        let input_line = if input.is_empty() {
            format!("{ERASE_LINE}  {YELLOW}> {DIM}Type something...{RESET}")
        } else {
            format!("{ERASE_LINE}  {YELLOW}> {RESET}{input}█")
        };
        out.push_str(&input_line);
        out.push_str("\r\n");
        rows += physical_row_count(&input_line, term_width);

        let hint_line = format!("{ERASE_LINE}  {DIM}[Enter submit  Esc back to list]{RESET}");
        out.push_str(&hint_line);
        out.push_str("\r\n");
        rows += physical_row_count(&hint_line, term_width);
    } else {
        let footer_line = format!(
            "{ERASE_LINE}  {DIM}[↑↓ select  Enter confirm  1-{} pick  {} custom  Esc skip]{RESET}",
            request.options.len(),
            none_num
        );
        out.push_str(&footer_line);
        out.push_str("\r\n");
        rows += physical_row_count(&footer_line, term_width);
    }

    (out, rows)
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
    let mut mode = AskUserMode::Selecting { selected: 0 };
    let mut prev_lines: usize = 0;
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            let term_width = terminal_width();
            let (output, line_count) = match &mode {
                AskUserMode::Typing { selected, input } => {
                    build_question_block_typing(request, *selected, term_width, input)
                }
                AskUserMode::Selecting { selected } => {
                    build_question_block(request, *selected, term_width)
                }
            };
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
                let (next_mode, action) = handle_key(request, mode, key.code, key.modifiers);
                mode = next_mode;

                match action {
                    AskUserAction::Redraw => {
                        needs_redraw = true;
                    }
                    AskUserAction::Submit(response) => {
                        clear_block(prev_lines);
                        match &response {
                            AskUserResponse::Skipped => {
                                print_result(&build_skipped());
                            }
                            AskUserResponse::Selected(label) => {
                                print_result(&build_confirmation(label));
                            }
                            AskUserResponse::Custom(text) => {
                                print_result(&build_confirmation(text));
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
