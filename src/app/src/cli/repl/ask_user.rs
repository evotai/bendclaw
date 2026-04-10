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
use rustyline::error::ReadlineError;

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

fn total_options(request: &AskUserRequest) -> usize {
    request.options.len() + 1
}

pub fn build_question_block(
    request: &AskUserRequest,
    selected: usize,
    term_width: usize,
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
    let is_none_selected = selected == none_idx;
    let marker = if is_none_selected { "›" } else { " " };
    let highlight = if is_none_selected { YELLOW } else { DIM };
    let none_line =
        format!("{ERASE_LINE}  {highlight}{marker} 0. None of the above (type your own){RESET}",);
    out.push_str(&none_line);
    out.push_str("\r\n");
    rows += physical_row_count(&none_line, term_width);

    out.push_str(&format!("{ERASE_LINE}\r\n"));
    rows += 1;

    let footer_line = format!(
        "{ERASE_LINE}  {DIM}[↑↓ select  Enter confirm  1-{} pick  0 custom  Esc skip]{RESET}",
        request.options.len()
    );
    out.push_str(&footer_line);
    out.push_str("\r\n");
    rows += physical_row_count(&footer_line, term_width);

    (out, rows)
}

pub fn build_confirmation(label: &str) -> String {
    format!("  {GREEN}✓ {label}{RESET}")
}

pub fn build_skipped() -> String {
    format!("  {DIM}— skipped{RESET}")
}

pub enum AskUserUiResult {
    Answer(AskUserResponse),
    ExitRun,
}

pub fn render_and_select(request: &AskUserRequest) -> std::io::Result<AskUserUiResult> {
    let total = total_options(request);
    let mut selected: usize = 0;
    let mut prev_lines: usize = 0;
    let mut needs_redraw = true;

    loop {
        if needs_redraw {
            let term_width = terminal_width();
            let (output, line_count) = build_question_block(request, selected, term_width);
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
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if selected > 0 {
                        selected -= 1;
                    } else {
                        selected = total - 1;
                    }
                    needs_redraw = true;
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1) % total;
                    needs_redraw = true;
                }

                KeyCode::Enter => {
                    let response = if selected < request.options.len() {
                        let label = request.options[selected].label.clone();
                        clear_block(prev_lines);
                        print_result(&build_confirmation(&label));
                        AskUserResponse::Selected(label)
                    } else {
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

                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    clear_block(prev_lines);
                    return Ok(AskUserUiResult::ExitRun);
                }

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

fn read_custom_input() -> std::io::Result<Option<String>> {
    let input = with_line_input(|| {
        let mut rl: rustyline::Editor<(), rustyline::history::DefaultHistory> =
            rustyline::Editor::new()
                .map_err(|e| std::io::Error::other(format!("rustyline init: {e}")))?;
        let prompt = format!("  {YELLOW}> {RESET}");
        match rl.readline(&prompt) {
            Ok(line) => Ok(line),
            Err(ReadlineError::Interrupted | ReadlineError::Eof) => Ok(String::new()),
            Err(e) => Err(std::io::Error::other(format!("readline: {e}"))),
        }
    })?;

    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        Ok(None)
    } else {
        Ok(Some(trimmed))
    }
}

fn with_line_input<T>(f: impl FnOnce() -> std::io::Result<T>) -> std::io::Result<T> {
    disable_raw_mode()?;
    let result = f();
    enable_raw_mode()?;
    result
}
