//! ANSI style definitions and dark/light theme detection.

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const ITALIC: &str = "\x1b[3m";
const UNDERLINE: &str = "\x1b[4m";

// Foreground colors
const FG_YELLOW: &str = "\x1b[33m";
const FG_CYAN: &str = "\x1b[36m";
const FG_BRIGHT_BLACK: &str = "\x1b[90m";

// ---------------------------------------------------------------------------
// Style
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Style {
    prefix: &'static str,
}

impl Style {
    const fn new(prefix: &'static str) -> Self {
        Self { prefix }
    }

    pub fn paint(&self, text: &str) -> String {
        if self.prefix.is_empty() {
            text.to_string()
        } else {
            format!("{}{}{}", self.prefix, text, RESET)
        }
    }
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

pub struct Theme {
    pub text: Style,
    pub bold: Style,
    pub italic: Style,
    pub bold_italic: Style,
    pub code_inline: Style,
    pub strikethrough: Style,
    pub underline: Style,
    pub link: Style,
    pub h1: Style,
    pub h2: Style,
    pub h3: Style,
    pub h4: Style,
    pub h5: Style,
    pub h6: Style,
    pub bullet: Style,
    pub list_number: Style,
    pub blockquote_border: Style,
    pub blockquote_text: Style,
    pub table_border: Style,
    pub table_header: Style,
    pub hr: Style,
    pub think_border: Style,
    pub think_text: Style,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            text: Style::new(""),
            bold: Style::new(BOLD),
            italic: Style::new(ITALIC),
            bold_italic: Style::new("\x1b[1;3m"),
            code_inline: Style::new(FG_YELLOW),
            strikethrough: Style::new("\x1b[2;9m"),
            underline: Style::new(UNDERLINE),
            link: Style::new("\x1b[4;36m"),
            h1: Style::new("\x1b[1;35m"),
            h2: Style::new("\x1b[1;34m"),
            h3: Style::new("\x1b[1;36m"),
            h4: Style::new("\x1b[1;32m"),
            h5: Style::new("\x1b[1;33m"),
            h6: Style::new("\x1b[1;37m"),
            bullet: Style::new(FG_CYAN),
            list_number: Style::new(FG_CYAN),
            blockquote_border: Style::new(FG_BRIGHT_BLACK),
            blockquote_text: Style::new("\x1b[2;3m"),
            table_border: Style::new(FG_BRIGHT_BLACK),
            table_header: Style::new(BOLD),
            hr: Style::new(FG_BRIGHT_BLACK),
            think_border: Style::new(FG_BRIGHT_BLACK),
            think_text: Style::new("\x1b[3;90m"),
        }
    }
}
