//! Terminal UI for the `ask_user` tool — renders structured questions and
//! collects user input via keyboard navigation.
//!
//! Pure rendering functions (`build_*`) are separated from IO functions
//! (`render_and_select`) for testability.

use std::io::Write;

use bend_engine::tools::AskUserRequest;
use bend_engine::tools::AskUserResponse;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;

use super::render::with_terminal;
use super::render::DIM;
use super::render::GREEN;
use super::render::RESET;
use super::render::YELLOW;

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Pure rendering (testable without a terminal)
// ---------------------------------------------------------------------------

/// Total option count including the fixed "None of the above" entry.
fn total_options(request: &AskUserRequest) -> usize {
    request.options.len() + 1
}

/// Build the full question block as a string. Returns `(output, line_count)`.
///
/// Layout:
/// ```text
///   ❓ <question>
///                          ← blank line
///   › 1. Label (Recommended)
///        Description text
///     2. Another option
///        Description text
///     0. None of the above (type your own)
///                          ← blank line
///   [↑↓ select  Enter confirm  1-N pick  0 custom  Esc skip]
/// ```
pub fn build_question_block(request: &AskUserRequest, selected: usize) -> (String, usize) {
    let mut out = String::new();
    let mut lines: usize = 0;

    // Question line
    out.push_str(&format!(
        "\r{ERASE_LINE}  {CYAN}❓ {BOLD}{}{RESET}\r\n",
        request.question
    ));
    lines += 1;

    // Blank line
    out.push_str(&format!("{ERASE_LINE}\r\n"));
    lines += 1;

    // Numbered options
    for (i, opt) in request.options.iter().enumerate() {
        let num = i + 1;
        let is_selected = i == selected;
        let marker = if is_selected { "›" } else { " " };
        let highlight = if is_selected { YELLOW } else { DIM };

        out.push_str(&format!(
            "{ERASE_LINE}  {highlight}{marker} {num}. {}{RESET}\r\n",
            opt.label
        ));
        lines += 1;

        out.push_str(&format!(
            "{ERASE_LINE}  {DIM}     {}{RESET}\r\n",
            opt.description
        ));
        lines += 1;
    }

    // "None of the above" option
    let none_idx = request.options.len();
    let is_none_selected = selected == none_idx;
    let marker = if is_none_selected { "›" } else { " " };
    let highlight = if is_none_selected { YELLOW } else { DIM };
    out.push_str(&format!(
        "{ERASE_LINE}  {highlight}{marker} 0. None of the above (type your own){RESET}\r\n",
    ));
    lines += 1;

    // Blank line
    out.push_str(&format!("{ERASE_LINE}\r\n"));
    lines += 1;

    // Footer hint
    out.push_str(&format!(
        "{ERASE_LINE}  {DIM}[↑↓ select  Enter confirm  1-{} pick  0 custom  Esc skip]{RESET}",
        request.options.len()
    ));
    lines += 1;

    (out, lines)
}

/// Build the confirmation line shown after the user selects an option.
pub fn build_confirmation(label: &str) -> String {
    format!("  {GREEN}✓ {label}{RESET}")
}

/// Build the skip line shown when the user presses Esc.
pub fn build_skipped() -> String {
    format!("  {DIM}— skipped{RESET}")
}

// ---------------------------------------------------------------------------
// Terminal interaction
// ---------------------------------------------------------------------------

/// Result from the ask_user UI interaction.
pub enum AskUserUiResult {
    /// User provided an answer (selected, custom, or skipped).
    Answer(AskUserResponse),
    /// User pressed Ctrl+C — caller should abort the entire run.
    ExitRun,
}

/// Render the question selector in the current raw-mode terminal and wait
/// for the user to pick an option, type custom input, or skip.
///
/// Caller must already be in raw mode (via `RawModeGuard`).
pub fn render_and_select(request: &AskUserRequest) -> std::io::Result<AskUserUiResult> {
    let total = total_options(request);
    let mut selected: usize = 0;
    let mut prev_lines: usize = 0;

    loop {
        // Erase previous frame
        if prev_lines > 0 {
            with_terminal(|stdout| {
                let _ = write!(stdout, "{}\r", cursor_up(prev_lines.saturating_sub(1)));
            });
        }

        let (output, line_count) = build_question_block(request, selected);
        with_terminal(|stdout| {
            let _ = write!(stdout, "{output}");
        });
        prev_lines = line_count;

        // Wait for key
        if !poll(std::time::Duration::from_millis(100))? {
            continue;
        }

        match read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                // Navigation
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    } else {
                        selected = total - 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % total;
                }

                // Confirm current selection
                KeyCode::Enter => {
                    let response = if selected < request.options.len() {
                        let label = request.options[selected].label.clone();
                        clear_block(prev_lines);
                        print_result(&build_confirmation(&label));
                        AskUserResponse::Selected(label)
                    } else {
                        // "None of the above" → custom input
                        clear_block(prev_lines);
                        match read_custom_input()? {
                            Some(text) => {
                                print_result(&build_confirmation(&text));
                                AskUserResponse::Custom(text)
                            }
                            None => {
                                print_result(&build_skipped());
                                AskUserResponse::Skipped
                            }
                        }
                    };
                    return Ok(AskUserUiResult::Answer(response));
                }

                // Quick-pick by number (1-N for options, 0 for custom)
                KeyCode::Char(ch @ '1'..='9') => {
                    let idx = (ch as usize) - ('1' as usize);
                    if idx < request.options.len() {
                        let label = request.options[idx].label.clone();
                        clear_block(prev_lines);
                        print_result(&build_confirmation(&label));
                        return Ok(AskUserUiResult::Answer(AskUserResponse::Selected(label)));
                    }
                }
                KeyCode::Char('0') => {
                    clear_block(prev_lines);
                    match read_custom_input()? {
                        Some(text) => {
                            print_result(&build_confirmation(&text));
                            return Ok(AskUserUiResult::Answer(AskUserResponse::Custom(text)));
                        }
                        None => {
                            print_result(&build_skipped());
                            return Ok(AskUserUiResult::Answer(AskUserResponse::Skipped));
                        }
                    }
                }

                // Ctrl+C — abort the entire run
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    clear_block(prev_lines);
                    return Ok(AskUserUiResult::ExitRun);
                }

                // Skip
                KeyCode::Esc => {
                    clear_block(prev_lines);
                    print_result(&build_skipped());
                    return Ok(AskUserUiResult::Answer(AskUserResponse::Skipped));
                }

                _ => {}
            },
            _ => {}
        }
    }
}

/// Clear the rendered question block.
fn clear_block(line_count: usize) {
    with_terminal(|stdout| {
        if line_count > 1 {
            let _ = write!(stdout, "{}\r", cursor_up(line_count.saturating_sub(1)));
        } else {
            let _ = write!(stdout, "\r");
        }
        for _ in 0..line_count {
            let _ = write!(stdout, "{ERASE_LINE}\r\n");
        }
        // Move back up
        if line_count > 0 {
            let _ = write!(stdout, "{}\r", cursor_up(line_count));
        }
    });
}

/// Print a single result line after the question is resolved.
fn print_result(text: &str) {
    with_terminal(|stdout| {
        let _ = write!(stdout, "{text}\r\n\r\n");
    });
}

/// Temporarily exit raw mode to read a line of free-form text.
/// Returns `None` if the user enters an empty string.
fn read_custom_input() -> std::io::Result<Option<String>> {
    // Exit raw mode so the user gets normal line editing
    let _ = disable_raw_mode();

    with_terminal(|stdout| {
        let _ = write!(stdout, "  {YELLOW}> {RESET}");
    });

    let mut input = String::new();
    let result = std::io::stdin().read_line(&mut input);

    // Re-enter raw mode
    let _ = enable_raw_mode();

    result?;
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}
