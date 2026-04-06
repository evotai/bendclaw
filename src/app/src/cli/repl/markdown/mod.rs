//! Streaming markdown renderer for REPL assistant output.
//!
//! Buffers incoming tokens, parses complete lines, and renders with
//! syntax highlighting and ANSI styling.

mod highlight;
mod render;
mod theme;

use std::io::Write;
use std::io::{self};
use std::sync::Arc;
use std::sync::Mutex;

use streamdown_core::ParseState;
use streamdown_parser::Parser;

use self::render::Renderer;
use super::render::with_terminal;
use super::spinner::SpinnerState;

// ---------------------------------------------------------------------------
// Repair — fix malformed markdown from LLM output
// ---------------------------------------------------------------------------

/// Split lines where a closing fence is glued to code content.
/// e.g. `}``` ` → [`}`, ```` ``` ````]
fn repair_line(line: &str, state: &ParseState) -> Vec<String> {
    if state.is_in_code() {
        let trimmed = line.trim_end();
        if let Some(stripped) = trimmed.strip_suffix("```") {
            if !stripped.trim().is_empty() {
                return vec![stripped.to_string(), "```".to_string()];
            }
        }
        if let Some(stripped) = trimmed.strip_suffix("~~~") {
            if !stripped.trim().is_empty() {
                return vec![stripped.to_string(), "~~~".to_string()];
            }
        }
    }
    vec![line.to_string()]
}

// ---------------------------------------------------------------------------
// SpinnerWriter — io::Write that coordinates with spinner
// ---------------------------------------------------------------------------

struct SpinnerWriter {
    spinner: Arc<Mutex<SpinnerState>>,
}

impl Write for SpinnerWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // Pause spinner before writing
        if let Ok(mut sp) = self.spinner.lock() {
            sp.clear_if_rendered();
            sp.hide();
        }

        let content = String::from_utf8_lossy(buf);
        // Normalize newlines for raw mode terminal
        let normalized = content.replace("\r\n", "\n").replace('\n', "\r\n");
        with_terminal(|stdout| {
            stdout.write_all(normalized.as_bytes())?;
            stdout.flush()
        })?;

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        with_terminal(|stdout| stdout.flush())
    }
}

// ---------------------------------------------------------------------------
// MarkdownStream — public API
// ---------------------------------------------------------------------------

/// Streaming markdown renderer.
///
/// Call `push()` with each token from the LLM, then `finish()` when done.
pub struct MarkdownStream {
    parser: Parser,
    renderer: Renderer<SpinnerWriter>,
    line_buffer: String,
    started: bool,
}

impl MarkdownStream {
    pub fn new(spinner: Arc<Mutex<SpinnerState>>) -> Self {
        let width = terminal_width();
        let writer = SpinnerWriter { spinner };
        Self {
            parser: Parser::new(),
            renderer: Renderer::new(writer, width),
            line_buffer: String::new(),
            started: false,
        }
    }

    /// Push a token (partial text) from the LLM stream.
    pub fn push(&mut self, token: &str) -> io::Result<()> {
        // Print the "•" prefix before the first content
        if !self.started {
            self.started = true;
            self.renderer.write_raw("\x1b[2m•\x1b[0m ")?;
        }

        self.line_buffer.push_str(token);

        while let Some(pos) = self.line_buffer.find('\n') {
            let line = self.line_buffer[..pos].to_string();

            for repaired in repair_line(&line, self.parser.state()) {
                for event in self.parser.parse_line(&repaired) {
                    self.renderer.render_event(&event)?;
                }
            }

            self.line_buffer = self.line_buffer[pos + 1..].to_string();
        }
        Ok(())
    }

    /// Finish rendering, flushing any remaining buffered content.
    pub fn finish(mut self) -> io::Result<()> {
        if !self.line_buffer.is_empty() {
            for repaired in repair_line(&self.line_buffer, self.parser.state()) {
                for event in self.parser.parse_line(&repaired) {
                    self.renderer.render_event(&event)?;
                }
            }
        }
        for event in self.parser.finalize() {
            self.renderer.render_event(&event)?;
        }
        Ok(())
    }
}

fn terminal_width() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}
