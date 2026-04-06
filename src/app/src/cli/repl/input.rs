//! Multiline input editor with Shift+Enter support.
//!
//! Uses crossterm with the kitty keyboard protocol so that Shift+Enter
//! inserts a newline while plain Enter submits.

use std::io::Write;

use crossterm::cursor;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::PushKeyboardEnhancementFlags;
use crossterm::event::{self, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags};
use crossterm::execute;
use crossterm::terminal::{self, ClearType};

/// Result of `read_input`.
pub enum InputResult {
    /// User submitted text (may contain newlines).
    Line(String),
    /// Ctrl-C
    Interrupted,
    /// Ctrl-D on empty input
    Eof,
}

/// Read user input with Shift+Enter for newline, Enter to submit.
///
/// `prompt` is printed before the first line; continuation lines are
/// indented with `"  ... "`.
pub fn read_input(prompt: &str) -> InputResult {
    // Print the prompt in normal mode first.
    print!("{prompt} ");
    let _ = std::io::stdout().flush();

    let enhanced = enable_enhanced_keys();
    let result = run_editor(prompt);
    if enhanced {
        disable_enhanced_keys();
    }
    result
}

// ---------------------------------------------------------------------------
// Enhanced keyboard mode (kitty protocol)
// ---------------------------------------------------------------------------

fn enable_enhanced_keys() -> bool {
    terminal::enable_raw_mode().ok();
    execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
    .is_ok()
}

fn disable_enhanced_keys() {
    let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    let _ = terminal::disable_raw_mode();
}

// ---------------------------------------------------------------------------
// Editor
// ---------------------------------------------------------------------------

fn run_editor(prompt: &str) -> InputResult {
    let mut buf = String::new();
    // Byte position of cursor within `buf`.
    let mut cursor_pos: usize = 0;

    loop {
        let ev = match event::read() {
            Ok(ev) => ev,
            Err(_) => return InputResult::Interrupted,
        };

        match ev {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                match key.code {
                    // Shift+Enter or Alt+Enter → insert newline
                    KeyCode::Enter
                        if key.modifiers.intersects(
                            KeyModifiers::SHIFT | KeyModifiers::ALT,
                        ) =>
                    {
                        buf.insert(cursor_pos, '\n');
                        cursor_pos += 1;
                        // Move to new line and print continuation prompt
                        write_raw("\r\n  ... ");
                    }

                    // Plain Enter → submit
                    KeyCode::Enter => {
                        write_raw("\r\n");
                        let _ = terminal::disable_raw_mode();
                        let _ = execute!(
                            std::io::stdout(),
                            PopKeyboardEnhancementFlags
                        );
                        // Re-enable will be skipped because we return.
                        return InputResult::Line(buf);
                    }

                    // Ctrl-C → interrupt
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        write_raw("\r\n");
                        return InputResult::Interrupted;
                    }

                    // Ctrl-D → EOF (only on empty buffer)
                    KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if buf.is_empty() {
                            write_raw("\r\n");
                            return InputResult::Eof;
                        }
                    }

                    // Backspace
                    KeyCode::Backspace => {
                        if cursor_pos > 0 {
                            // Find the previous char boundary
                            let prev = prev_char_boundary(&buf, cursor_pos);
                            let removed = &buf[prev..cursor_pos];
                            if removed == "\n" {
                                // Went back across a newline — need full redraw
                                buf.remove(prev);
                                cursor_pos = prev;
                                redraw(prompt, &buf, cursor_pos);
                            } else {
                                buf.drain(prev..cursor_pos);
                                cursor_pos = prev;
                                // Simple inline redraw: move back, rewrite rest of line, clear
                                let after = current_line_after(&buf, cursor_pos);
                                write_raw("\x08");
                                write_raw(after);
                                write_raw(" "); // clear last char
                                // Move cursor back
                                let back = after.len() + 1;
                                if back > 0 {
                                    move_cursor_left(back);
                                }
                            }
                        }
                    }

                    // Regular character
                    KeyCode::Char(c) => {
                        buf.insert(cursor_pos, c);
                        cursor_pos += c.len_utf8();
                        // Write char and any remaining text on this line
                        let after = current_line_after(&buf, cursor_pos);
                        let mut out = String::with_capacity(c.len_utf8() + after.len());
                        out.push(c);
                        out.push_str(after);
                        write_raw(&out);
                        if !after.is_empty() {
                            move_cursor_left(after.len());
                        }
                    }

                    // Left arrow
                    KeyCode::Left => {
                        if cursor_pos > 0 {
                            let prev = prev_char_boundary(&buf, cursor_pos);
                            if &buf[prev..cursor_pos] == "\n" {
                                cursor_pos = prev;
                                redraw(prompt, &buf, cursor_pos);
                            } else {
                                cursor_pos = prev;
                                move_cursor_left(1);
                            }
                        }
                    }

                    // Right arrow
                    KeyCode::Right => {
                        if cursor_pos < buf.len() {
                            let next = next_char_boundary(&buf, cursor_pos);
                            if &buf[cursor_pos..next] == "\n" {
                                cursor_pos = next;
                                redraw(prompt, &buf, cursor_pos);
                            } else {
                                cursor_pos = next;
                                move_cursor_right(1);
                            }
                        }
                    }

                    // Ctrl-A → beginning of current line
                    KeyCode::Home | KeyCode::Char('a')
                        if key.code == KeyCode::Home
                            || key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        let line_start = buf[..cursor_pos].rfind('\n').map_or(0, |i| i + 1);
                        if cursor_pos != line_start {
                            cursor_pos = line_start;
                            redraw(prompt, &buf, cursor_pos);
                        }
                    }

                    // Ctrl-E → end of current line
                    KeyCode::End | KeyCode::Char('e')
                        if key.code == KeyCode::End
                            || key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        let line_end = buf[cursor_pos..]
                            .find('\n')
                            .map_or(buf.len(), |i| cursor_pos + i);
                        if cursor_pos != line_end {
                            cursor_pos = line_end;
                            redraw(prompt, &buf, cursor_pos);
                        }
                    }

                    // Ctrl-U → kill line (clear current line before cursor)
                    KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let line_start = buf[..cursor_pos].rfind('\n').map_or(0, |i| i + 1);
                        if cursor_pos > line_start {
                            buf.drain(line_start..cursor_pos);
                            cursor_pos = line_start;
                            redraw(prompt, &buf, cursor_pos);
                        }
                    }

                    // Ctrl-K → kill to end of line
                    KeyCode::Char('k') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        let line_end = buf[cursor_pos..]
                            .find('\n')
                            .map_or(buf.len(), |i| cursor_pos + i);
                        if cursor_pos < line_end {
                            buf.drain(cursor_pos..line_end);
                            redraw(prompt, &buf, cursor_pos);
                        }
                    }

                    // Ctrl-W → delete word backward
                    KeyCode::Char('w') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        if cursor_pos > 0 {
                            let mut end = cursor_pos;
                            // Skip whitespace
                            while end > 0 && buf.as_bytes()[end - 1] == b' ' {
                                end -= 1;
                            }
                            // Skip word chars
                            while end > 0
                                && buf.as_bytes()[end - 1] != b' '
                                && buf.as_bytes()[end - 1] != b'\n'
                            {
                                end -= 1;
                            }
                            buf.drain(end..cursor_pos);
                            cursor_pos = end;
                            redraw(prompt, &buf, cursor_pos);
                        }
                    }

                    _ => {}
                }
            }
            Event::Paste(text) => {
                buf.insert_str(cursor_pos, &text);
                cursor_pos += text.len();
                redraw(prompt, &buf, cursor_pos);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Full redraw of the input area.
fn redraw(prompt: &str, buf: &str, cursor_pos: usize) {
    let mut out = std::io::stdout();

    // Count how many lines we need to move up from current position
    // to get back to the first line.
    let lines: Vec<&str> = buf.split('\n').collect();
    let total_lines = lines.len();

    // Figure out which line the cursor was on before (approximate: go to top)
    // Move to column 0 of the first line
    // We'll move up total_lines-1 to be safe, then rewrite everything.
    if total_lines > 1 {
        // Move up to the first line — we might be on any line
        let _ = execute!(out, cursor::MoveToColumn(0));
        // Move up enough lines (worst case: we're on the last line)
        for _ in 0..total_lines {
            let _ = execute!(out, cursor::MoveUp(1));
        }
        // Now move down one to be on the first line
        let _ = execute!(out, cursor::MoveDown(1));
    }
    let _ = execute!(out, cursor::MoveToColumn(0));

    // Clear from here to end of screen
    let _ = execute!(out, terminal::Clear(ClearType::FromCursorDown));

    // Rewrite prompt + buffer
    let cont = "  ... ";
    let prompt_display = format!("{prompt} ");
    let mut written = 0;
    for (i, line) in lines.iter().enumerate() {
        if i == 0 {
            write_raw(&prompt_display);
        } else {
            write_raw("\r\n");
            write_raw(cont);
        }
        write_raw(line);
        written += line.len();
        if i < lines.len() - 1 {
            written += 1; // for '\n'
        }
    }
    let _ = written;

    // Now position the cursor at cursor_pos.
    // Figure out which line and column cursor_pos is on.
    let (cursor_line, cursor_col) = line_col_of(buf, cursor_pos);
    let lines_from_end = (total_lines - 1) - cursor_line;
    if lines_from_end > 0 {
        let _ = execute!(out, cursor::MoveUp(lines_from_end as u16));
    }
    let prefix_len = if cursor_line == 0 {
        prompt_display.len()
    } else {
        cont.len()
    };
    let _ = execute!(out, cursor::MoveToColumn((prefix_len + cursor_col) as u16));
    let _ = out.flush();
}

fn line_col_of(buf: &str, pos: usize) -> (usize, usize) {
    let before = &buf[..pos];
    let line = before.matches('\n').count();
    let col = before.rfind('\n').map_or(pos, |i| pos - i - 1);
    (line, col)
}

fn current_line_after<'a>(buf: &'a str, pos: usize) -> &'a str {
    let end = buf[pos..].find('\n').map_or(buf.len(), |i| pos + i);
    &buf[pos..end]
}

fn prev_char_boundary(s: &str, pos: usize) -> usize {
    let mut p = pos - 1;
    while !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

fn next_char_boundary(s: &str, pos: usize) -> usize {
    let mut p = pos + 1;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p.min(s.len())
}

fn write_raw(s: &str) {
    let mut out = std::io::stdout();
    let _ = out.write_all(s.as_bytes());
    let _ = out.flush();
}

fn move_cursor_left(n: usize) {
    if n > 0 {
        let _ = execute!(std::io::stdout(), cursor::MoveLeft(n as u16));
    }
}

fn move_cursor_right(n: usize) {
    if n > 0 {
        let _ = execute!(std::io::stdout(), cursor::MoveRight(n as u16));
    }
}
