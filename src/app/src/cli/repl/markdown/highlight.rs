//! Syntax highlighting for code blocks using syntect.

use syntect::easy::HighlightLines;
use syntect::highlighting::ThemeSet;
use syntect::parsing::SyntaxSet;
use syntect::util::as_24_bit_terminal_escaped;

const RESET: &str = "\x1b[0m";

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl Default for Highlighter {
    fn default() -> Self {
        Self {
            syntax_set: SyntaxSet::load_defaults_newlines(),
            theme_set: ThemeSet::load_defaults(),
        }
    }
}

impl Highlighter {
    /// Highlight a single line of code, returning ANSI-escaped string.
    pub fn highlight_line(&self, line: &str, language: Option<&str>) -> String {
        let syntax = language
            .and_then(|lang| self.syntax_set.find_syntax_by_token(lang))
            .unwrap_or_else(|| self.syntax_set.find_syntax_plain_text());

        let theme = &self.theme_set.themes["base16-ocean.dark"];
        let mut hl = HighlightLines::new(syntax, theme);

        match hl.highlight_line(line, &self.syntax_set) {
            Ok(ranges) => format!("{}{}", as_24_bit_terminal_escaped(&ranges, false), RESET),
            Err(_) => line.to_string(),
        }
    }
}
