use std::io::Write;
use std::time::Instant;

use super::render::human_duration;
use super::render::with_terminal;
use super::render::DIM;
use super::render::GRAY;
use super::render::RED;
use super::render::RESET;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Spinner glyphs — gentle pulsing dot to indicate activity.
const GLYPHS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Terminal tab title glyphs — soft three-state activity dots.
const TITLE_GLYPHS: &[&str] = &["·", "•", "·"];

/// Obscure database-flavored verbs for the spinner.
const VERBS: &[&str] = &[
    "Defragmenting",
    "Denormalizing",
    "Sharding",
    "Vacuuming",
    "Reindexing",
    "Compacting",
    "Coalescing",
    "Partitioning",
    "Materializing",
    "Checkpointing",
    "Tombstoning",
    "Backfilling",
    "Rehashing",
    "Journaling",
    "Snapshotting",
    "Gossipping",
    "Quiescing",
    "Fencing",
    "Spilling",
    "Compressing",
];

/// ANSI: bright/bold white for glimmer highlight.
const BRIGHT: &str = "\x1b[97m";

/// Maximum characters per progress line before truncation.
const MAX_LINE_WIDTH: usize = 120;

/// Maximum number of progress tail lines shown above the spinner.
const MAX_PROGRESS_LINES: usize = 5;

/// Stalled threshold: 3 seconds with no new tokens.
const STALLED_THRESHOLD_MS: u128 = 3_000;

/// Show token count after this many ms.
const SHOW_TOKENS_AFTER_MS: u128 = 30_000;

/// When tokens are actively streaming, only advance the spinner frame
/// every N render calls. This keeps the animation calm while markdown
/// is being printed (the user already sees activity via the text).
const STREAMING_FRAME_DIVISOR: usize = 4;

/// Tokens are considered "actively streaming" if the last token arrived
/// within this many milliseconds.
const STREAMING_RECENCY_MS: u128 = 500;

// ---------------------------------------------------------------------------
// SpinnerPhase
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum SpinnerPhase {
    Verb,
    Tool { name: String },
    ToolProgress { name: String, text: String },
    Hidden,
}

// ---------------------------------------------------------------------------
// SpinnerState
// ---------------------------------------------------------------------------

pub struct SpinnerState {
    phase: SpinnerPhase,
    run_started_at: Instant,
    last_token_at: Instant,
    active: bool,
    rendered: bool,
    paused: bool,
    frame: usize,
    tick: usize,
    verb: String,
    response_tokens: u64,
    glimmer_pos: i32,
    /// Tail lines from the latest ToolProgress text, shown above the spinner line.
    progress_lines: Vec<String>,
    /// Total lines rendered in the last frame (progress lines + spinner line).
    /// Used by `clear_if_rendered` to cursor-up and erase the whole block.
    rendered_lines: usize,
}

impl Default for SpinnerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SpinnerState {
    pub fn new() -> Self {
        let now = Instant::now();
        Self {
            phase: SpinnerPhase::Hidden,
            run_started_at: now,
            last_token_at: now,
            active: false,
            rendered: false,
            paused: false,
            frame: 0,
            tick: 0,
            verb: pick_verb(),
            response_tokens: 0,
            glimmer_pos: -2,
            progress_lines: Vec::new(),
            rendered_lines: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        let now = Instant::now();
        self.active = true;
        self.rendered = false;
        self.paused = false;
        self.phase = SpinnerPhase::Verb;
        self.run_started_at = now;
        self.last_token_at = now;
        self.frame = 0;
        self.tick = 0;
        self.verb = pick_verb();
        self.response_tokens = 0;
        self.glimmer_pos = -2;
        self.progress_lines.clear();
        self.rendered_lines = 0;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.rendered = false;
        self.paused = false;
        self.phase = SpinnerPhase::Hidden;
        with_terminal(|stdout| {
            let _ = write!(stdout, "\x1b]0;BendClaw\x07");
        });
    }

    pub fn set_tool(&mut self, name: &str) {
        self.paused = false;
        self.progress_lines.clear();
        self.phase = SpinnerPhase::Tool {
            name: name.to_string(),
        };
    }

    pub fn set_progress(&mut self, text: &str) {
        let lines: Vec<&str> = text.lines().collect();
        self.progress_lines = lines
            .iter()
            .rev()
            .take(MAX_PROGRESS_LINES)
            .rev()
            .map(|l| truncate_line(l, MAX_LINE_WIDTH).to_string())
            .collect();
        // Preserve the tool name from the current phase
        let name = match &self.phase {
            SpinnerPhase::Tool { name } | SpinnerPhase::ToolProgress { name, .. } => name.clone(),
            _ => String::new(),
        };
        self.phase = SpinnerPhase::ToolProgress {
            name,
            text: text.to_string(),
        };
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn restore_verb(&mut self) {
        self.paused = false;
        self.progress_lines.clear();
        self.phase = SpinnerPhase::Verb;
    }

    pub fn add_tokens(&mut self, count: u64) {
        self.response_tokens += count;
        self.last_token_at = Instant::now();
    }

    /// Render one animation frame to stdout. Called from the 80ms poll loop.
    ///
    /// For `ToolProgress`, renders progress tail lines above the spinner line
    /// as a single multi-line block, using cursor-up to overwrite the previous frame.
    /// When paused, rendering is skipped entirely.
    pub fn render_frame(&mut self) {
        if !self.active || self.paused || matches!(self.phase, SpinnerPhase::Hidden) {
            return;
        }

        self.tick += 1;

        // Tokens arrived recently → slow down the animation.
        let streaming = self.last_token_at.elapsed().as_millis() < STREAMING_RECENCY_MS
            && self.response_tokens > 0;
        if streaming && !self.tick.is_multiple_of(STREAMING_FRAME_DIVISOR) {
            return;
        }

        let glyph = GLYPHS[self.frame % GLYPHS.len()];
        let title_glyph = TITLE_GLYPHS[self.frame % TITLE_GLYPHS.len()];
        self.frame += 1;

        let elapsed_ms = self.run_started_at.elapsed().as_millis() as u64;
        let stalled = self.last_token_at.elapsed().as_millis() > STALLED_THRESHOLD_MS;

        // Build status: (elapsed) or (elapsed · Nk tokens)
        let mut status = human_duration(elapsed_ms);
        if self.run_started_at.elapsed().as_millis() > SHOW_TOKENS_AFTER_MS
            && self.response_tokens > 0
        {
            let token_display = format_tokens(self.response_tokens);
            status = format!("{status} · {token_display} tokens");
        }

        let glyph_color = if stalled { RED } else { GRAY };

        // ToolProgress: multi-line block (progress lines above, spinner line below)
        if let SpinnerPhase::ToolProgress { name, .. } = &self.phase {
            let (output, new_lines) = build_progress_frame(
                &self.progress_lines,
                self.rendered_lines,
                glyph,
                glyph_color,
                name,
                &status,
            );

            with_terminal(|stdout| {
                let _ = write!(stdout, "{output}");
                let _ = write!(stdout, "\x1b]0;{title_glyph} BendClaw\x07");
            });

            self.rendered_lines = new_lines;
            self.rendered = true;
            return;
        }

        // Normal single-line spinner (Verb / Tool phases)
        let message = self.message_text();

        // Glimmer: advance position, wrap around
        let msg_len = message.chars().count() as i32;
        self.glimmer_pos += 1;
        if self.glimmer_pos > msg_len + 10 {
            self.glimmer_pos = -2;
        }

        // Build the message with glimmer effect
        let rendered_msg = if stalled {
            format!("{RED}{message}{RESET}")
        } else {
            render_glimmer(&message, self.glimmer_pos)
        };

        with_terminal(|stdout| {
            let _ = write!(
                stdout,
                "\r{glyph_color}{glyph}{RESET} {rendered_msg} {DIM}({status}) · esc to interrupt{RESET}\x1b[K"
            );
            let _ = write!(stdout, "\x1b]0;{title_glyph} BendClaw\x07");
        });
        self.rendered_lines = 1;
        self.rendered = true;
    }

    /// Clear the spinner region (single or multi-line) if it was rendered.
    pub fn clear_if_rendered(&mut self) {
        if !self.rendered {
            return;
        }

        let seq = build_clear_sequence(self.rendered_lines);
        with_terminal(|stdout| {
            let _ = write!(stdout, "{seq}");
        });

        self.rendered = false;
        self.rendered_lines = 0;
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

impl SpinnerState {
    fn message_text(&self) -> String {
        match &self.phase {
            SpinnerPhase::Verb => format!("{}…", self.verb),
            SpinnerPhase::Tool { name } => format!("Running {name}…"),
            SpinnerPhase::ToolProgress { .. } | SpinnerPhase::Hidden => String::new(),
        }
    }
}

fn pick_verb() -> String {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos() as usize)
        .unwrap_or(0);
    VERBS[seed % VERBS.len()].to_string()
}

fn format_tokens(count: u64) -> String {
    if count >= 1000 {
        format!("{:.1}k", count as f64 / 1000.0)
    } else {
        count.to_string()
    }
}

/// Render message with a glimmer highlight sweeping left to right.
/// Characters at glimmer_pos ± 1 are rendered in bright white.
fn render_glimmer(message: &str, glimmer_pos: i32) -> String {
    let mut result = String::new();
    let shimmer_start = glimmer_pos - 1;
    let shimmer_end = glimmer_pos + 1;

    for (i, ch) in message.chars().enumerate() {
        let pos = i as i32;
        if pos >= shimmer_start && pos <= shimmer_end {
            result.push_str(BRIGHT);
            result.push(ch);
            result.push_str(RESET);
        } else {
            result.push_str(GRAY);
            result.push(ch);
            result.push_str(RESET);
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Accessors for tests
// ---------------------------------------------------------------------------

impl SpinnerState {
    pub fn phase(&self) -> &SpinnerPhase {
        &self.phase
    }

    pub fn frame_index(&self) -> usize {
        self.frame
    }

    pub fn current_glyph(&self) -> &str {
        GLYPHS[self.frame % GLYPHS.len()]
    }

    /// Advance the frame counter.
    pub fn advance_frame(&mut self) {
        self.frame += 1;
    }

    /// Elapsed milliseconds since the run started.
    pub fn elapsed_ms(&self) -> u64 {
        self.run_started_at.elapsed().as_millis() as u64
    }

    pub fn glyph_count() -> usize {
        GLYPHS.len()
    }

    /// Number of lines currently rendered (progress lines + spinner line).
    pub fn rendered_line_count(&self) -> usize {
        self.rendered_lines
    }

    /// Current progress tail lines.
    pub fn progress_lines(&self) -> &[String] {
        &self.progress_lines
    }
}

impl SpinnerPhase {
    pub fn is_verb(&self) -> bool {
        matches!(self, Self::Verb)
    }

    pub fn is_tool(&self) -> bool {
        matches!(self, Self::Tool { .. })
    }

    pub fn is_hidden(&self) -> bool {
        matches!(self, Self::Hidden)
    }
}

// ---------------------------------------------------------------------------
// Render sequence builders (pure functions, testable without a terminal)
// ---------------------------------------------------------------------------

/// ANSI sequence: move cursor up N lines.
const CUR_UP: &str = "\x1b[";
const CUR_UP_SUFFIX: &str = "A";
/// ANSI sequence: erase from cursor to end of line.
const ERASE_LINE: &str = "\x1b[K";

/// Build the byte sequence for a multi-line progress frame.
///
/// Layout (raw-mode safe, uses `\r\n`):
///   progress_line_0  (with \r\n)
///   progress_line_1  (with \r\n)
///   ...              (padding blank lines if block shrank)
///   spinner_line     (NO trailing newline — cursor stays here)
///
/// The spinner line is pinned: it never moves up when progress lines shrink.
/// The block size is `max(prev_rendered_lines, progress_lines + 1)`.
///
/// Returns `(output, new_rendered_lines)`.
pub fn build_progress_frame(
    progress_lines: &[String],
    prev_rendered_lines: usize,
    glyph: &str,
    glyph_color: &str,
    tool_name: &str,
    status: &str,
) -> (String, usize) {
    let separator = if progress_lines.is_empty() { 0 } else { 1 };
    let content_lines = progress_lines.len() + separator + 1; // progress + gap + spinner
                                                              // Pin: block never shrinks, so spinner stays on the same terminal row.
    let total_lines = content_lines.max(prev_rendered_lines);
    let padding = total_lines - content_lines; // blank lines between progress and spinner

    let mut out = String::new();

    // Move cursor to the start of the previously rendered block.
    if prev_rendered_lines > 1 {
        out.push_str(&format!(
            "{CUR_UP}{}{CUR_UP_SUFFIX}",
            prev_rendered_lines - 1
        ));
    }
    out.push('\r');

    // Progress lines
    for line in progress_lines {
        out.push_str(&format!("{ERASE_LINE}{DIM}  {line}{RESET}\r\n"));
    }

    // Blank line separating progress from spinner (only when progress is shown)
    if separator > 0 {
        out.push_str(&format!("{ERASE_LINE}\r\n"));
    }

    // Padding blank lines (keeps spinner pinned when progress shrinks)
    for _ in 0..padding {
        out.push_str(&format!("{ERASE_LINE}\r\n"));
    }

    // Spinner line (no trailing newline — cursor stays here)
    let tool_label = if tool_name.is_empty() {
        "Running…".to_string()
    } else {
        format!("Running {tool_name}…")
    };
    out.push_str(&format!(
        "{ERASE_LINE}{glyph_color}{glyph}{RESET} {DIM}{tool_label}{RESET} {DIM}({status}) · esc to interrupt{RESET}{ERASE_LINE}"
    ));

    (out, total_lines)
}

/// Build the byte sequence to clear a rendered spinner region.
///
/// For multi-line: cursor-up to top, clear each line, cursor-up back.
/// For single-line: just `\r\x1b[K`.
pub fn build_clear_sequence(rendered_lines: usize) -> String {
    if rendered_lines <= 1 {
        return format!("\r{ERASE_LINE}");
    }
    let mut out = String::new();
    // Cursor is on the spinner line (last line, no trailing \n).
    out.push_str(&format!("{CUR_UP}{}{CUR_UP_SUFFIX}", rendered_lines - 1));
    for _ in 0..rendered_lines {
        out.push_str(&format!("\r{ERASE_LINE}\r\n"));
    }
    // Move back up so cursor is at the start
    out.push_str(&format!("{CUR_UP}{rendered_lines}{CUR_UP_SUFFIX}"));
    out
}
fn truncate_line(line: &str, max_width: usize) -> &str {
    if line.len() <= max_width {
        return line;
    }
    let mut end = max_width;
    while end > 0 && !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}
