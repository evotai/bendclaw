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

/// Terminal tab title prefix — soft three-state activity dots.
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

/// Stalled threshold: 3 seconds with no new tokens.
const STALLED_THRESHOLD_MS: u128 = 3_000;

/// Show token count after this many ms.
const SHOW_TOKENS_AFTER_MS: u128 = 30_000;

// ---------------------------------------------------------------------------
// SpinnerPhase
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub enum SpinnerPhase {
    Verb,
    Tool { name: String },
    ToolProgress { text: String },
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
    frame: usize,
    verb: String,
    response_tokens: u64,
    glimmer_pos: i32,
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
            frame: 0,
            verb: pick_verb(),
            response_tokens: 0,
            glimmer_pos: -2,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn activate(&mut self) {
        let now = Instant::now();
        self.active = true;
        self.rendered = false;
        self.phase = SpinnerPhase::Verb;
        self.run_started_at = now;
        self.last_token_at = now;
        self.frame = 0;
        self.verb = pick_verb();
        self.response_tokens = 0;
        self.glimmer_pos = -2;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.rendered = false;
        self.phase = SpinnerPhase::Hidden;
        // Reset terminal tab title
        with_terminal(|stdout| {
            let _ = write!(stdout, "\x1b]0;BendClaw\x07");
        });
    }

    pub fn set_tool(&mut self, name: &str) {
        self.phase = SpinnerPhase::Tool {
            name: name.to_string(),
        };
    }

    pub fn set_progress(&mut self, text: &str) {
        self.phase = SpinnerPhase::ToolProgress {
            text: text.to_string(),
        };
    }

    pub fn restore_verb(&mut self) {
        self.phase = SpinnerPhase::Verb;
    }

    pub fn add_tokens(&mut self, count: u64) {
        self.response_tokens += count;
        self.last_token_at = Instant::now();
    }

    /// Render one animation frame to stdout. Called from the 80ms poll loop.
    pub fn render_frame(&mut self) {
        if !self.active || matches!(self.phase, SpinnerPhase::Hidden) {
            return;
        }

        let glyph = GLYPHS[self.frame % GLYPHS.len()];
        let title_glyph = TITLE_GLYPHS[self.frame % TITLE_GLYPHS.len()];
        self.frame += 1;

        let message = self.message_text();
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

        // Glimmer: advance position, wrap around
        let msg_len = message.chars().count() as i32;
        self.glimmer_pos += 1;
        if self.glimmer_pos > msg_len + 10 {
            self.glimmer_pos = -2;
        }

        // Pick colors based on stalled state
        let (glyph_color, msg_color, msg_reset) = if stalled {
            (RED, RED, RESET)
        } else {
            (GRAY, "", "")
        };

        // Build the message with glimmer effect
        let rendered_msg = if stalled {
            format!("{msg_color}{message}{msg_reset}")
        } else {
            render_glimmer(&message, self.glimmer_pos)
        };

        with_terminal(|stdout| {
            let _ = write!(
                stdout,
                "\r{glyph_color}{glyph}{RESET} {rendered_msg} {DIM}({status}) · esc to interrupt{RESET}\x1b[K"
            );
            // Set terminal tab title to show activity state.
            let _ = write!(stdout, "\x1b]0;{title_glyph} BendClaw\x07");
        });
        self.rendered = true;
    }

    /// Clear the spinner line only if the spinner was actually rendered.
    /// This prevents erasing real output when the spinner is hidden.
    pub fn clear_if_rendered(&mut self) {
        if self.rendered {
            with_terminal(|stdout| {
                let _ = write!(stdout, "\r\x1b[K");
            });
            self.rendered = false;
        }
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
            SpinnerPhase::ToolProgress { text } => format!("{text}…"),
            SpinnerPhase::Hidden => String::new(),
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

    pub fn glyph_count() -> usize {
        GLYPHS.len()
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
