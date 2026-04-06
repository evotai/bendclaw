use std::io::Stdout;
use std::io::Write;
use std::sync::Mutex;
use std::sync::OnceLock;

pub use crate::cli::format::format_tool_input;
pub use crate::cli::format::summarize_inline;
pub use crate::cli::format::truncate;
use crate::protocol::TranscriptItem;
use crate::protocol::UsageSummary;

pub const RESET: &str = "\x1b[0m";
pub const BOLD: &str = "\x1b[1m";
pub const DIM: &str = "\x1b[2m";
pub const GREEN: &str = "\x1b[32m";
pub const YELLOW: &str = "\x1b[33m";
pub const RED: &str = "\x1b[31m";
pub const BLACK: &str = "\x1b[30m";
pub const WHITE: &str = "\x1b[37m";
pub const GRAY: &str = "\x1b[90m";
pub const BG_TOOL: &str = "\x1b[48;2;245;197;66m";
pub const BG_OK: &str = "\x1b[48;2;133;220;140m";
pub const BG_ERR: &str = "\x1b[48;2;157;57;57m";

static TERMINAL_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub fn with_terminal<T>(render: impl FnOnce(&mut Stdout) -> T) -> T {
    let _guard = TERMINAL_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut stdout = std::io::stdout();
    let result = render(&mut stdout);
    let _ = stdout.flush();
    result
}

fn write_terminal(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    with_terminal(|stdout| {
        let _ = write!(stdout, "{normalized}");
    });
}

// ---------------------------------------------------------------------------
// Low-level terminal output
// ---------------------------------------------------------------------------

pub fn normalize_terminal_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

pub fn terminal_write(text: &str) {
    write_terminal(text);
}

pub fn terminal_writeln(text: &str) {
    terminal_write(text);
    terminal_write("\r\n");
}

pub fn terminal_prefixed_writeln(text: &str) {
    let normalized = normalize_terminal_newlines(text);
    let output = format!("{DIM}•{RESET} {normalized}\r\n");
    write_terminal(&output);
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

pub fn human_duration(duration_ms: u64) -> String {
    if duration_ms >= 1000 {
        format!("{:.1}s", duration_ms as f64 / 1000.0)
    } else {
        format!("{duration_ms}ms")
    }
}

pub fn build_run_summary(usage: &UsageSummary, turn_count: u32, duration_ms: u64) -> String {
    let total_tokens = usage.input + usage.output;

    [
        format!("run {}", human_duration(duration_ms)),
        format!("turns {}", turn_count),
        format!("tokens {}", total_tokens),
    ]
    .join("  ·  ")
}

// ---------------------------------------------------------------------------
// Transcript rendering
// ---------------------------------------------------------------------------

pub fn print_transcript_messages(items: &[TranscriptItem]) {
    for item in items {
        match item {
            TranscriptItem::User { text } => {
                if !text.trim().is_empty() {
                    println!("{YELLOW}> {RESET}{}", text.trim());
                    println!();
                }
            }
            TranscriptItem::Assistant {
                text, tool_calls, ..
            } => {
                if !text.trim().is_empty() {
                    terminal_prefixed_writeln(text.trim());
                    terminal_writeln("");
                }
                for tc in tool_calls {
                    print_tool_call(&tc.name, &tc.input);
                }
            }
            TranscriptItem::ToolResult {
                tool_name,
                content,
                is_error,
                ..
            } => {
                let title = if *is_error {
                    format!("{tool_name} failed")
                } else {
                    format!("{tool_name} completed")
                };
                print_badge_line(&title, true, !is_error);
                terminal_writeln(&format!(
                    "{}  {}{}",
                    if *is_error { RED } else { GREEN },
                    summarize_inline(content, 160),
                    RESET
                ));
                terminal_writeln("");
            }
            _ => {}
        }
    }
}

pub fn print_tool_call(name: &str, input: &serde_json::Value) {
    let (title, lines) = tool_call_message(name, input);
    print_badge_line(&title, false, false);
    for line in lines {
        terminal_writeln(&format!("{GRAY}  {line}{RESET}"));
    }
    terminal_writeln("");
}

pub fn print_tool_result(
    tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) {
    let title = if is_error {
        format!("{tool_name} failed")
    } else {
        format!("{tool_name} completed")
    };
    let line = tool_result_line(tool_name, content, is_error, tool_call);
    print_badge_line(&title, true, !is_error);
    terminal_writeln(&format!(
        "{}  {}{}",
        if is_error { RED } else { GREEN },
        line,
        RESET
    ));
    terminal_writeln("");
}

pub fn print_badge_line(title: &str, is_result: bool, ok: bool) {
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

pub fn tool_call_message(name: &str, input: &serde_json::Value) -> (String, Vec<String>) {
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

pub fn tool_result_line(
    _tool_name: &str,
    content: &str,
    is_error: bool,
    tool_call: Option<&ToolCallSummary>,
) -> String {
    if !is_error {
        if let Some(tc) = tool_call {
            if tc.name.to_lowercase().contains("read") {
                return format!("Result: {}", tc.summary);
            }
        }
    }
    if content.trim().is_empty() {
        if is_error {
            "Result: tool returned an error".into()
        } else {
            "Result: completed".into()
        }
    } else {
        format!("Result: {}", summarize_inline(content, 160))
    }
}

pub fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
}

/// Minimal summary of a tool call used for result display.
pub struct ToolCallSummary {
    pub name: String,
    pub summary: String,
}
