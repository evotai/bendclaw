//! Renders ParseEvent into ANSI terminal output.

use std::io::Write;
use std::io::{self};

use streamdown_parser::ParseEvent;

use super::highlight::Highlighter;
use super::linkify::format_hyperlink;
use super::linkify::linkify_issue_refs;
use super::list_state::ListState;
use super::table;
use super::theme::Theme;

pub struct Renderer<W: Write> {
    writer: W,
    width: usize,
    theme: Theme,
    current_language: Option<String>,
    in_blockquote: bool,
    blockquote_depth: usize,
    table_rows: Vec<Vec<String>>,
    list_state: ListState,
}

impl<W: Write> Renderer<W> {
    pub fn new(writer: W, width: usize) -> Self {
        Self {
            writer,
            width,
            theme: Theme::default(),
            current_language: None,
            in_blockquote: false,
            blockquote_depth: 0,
            table_rows: Vec::new(),
            list_state: ListState::default(),
        }
    }

    pub fn render_event(&mut self, event: &ParseEvent) -> io::Result<()> {
        // Reset list state when a non-list event breaks the context.
        if ListState::should_reset(event) {
            self.list_state.reset();
        }

        match event {
            // --- Inline ---
            ParseEvent::Text(text) => self.write(
                &self
                    .theme
                    .text
                    .paint(&linkify_issue_refs(text, &self.theme.link)),
            )?,
            ParseEvent::Bold(text) => self.write(&self.theme.bold.paint(text))?,
            ParseEvent::Italic(text) => self.write(&self.theme.italic.paint(text))?,
            ParseEvent::BoldItalic(text) => self.write(&self.theme.bold_italic.paint(text))?,
            ParseEvent::InlineCode(text) => self.write(&self.theme.code_inline.paint(text))?,
            ParseEvent::Strikeout(text) => self.write(&self.theme.strikethrough.paint(text))?,
            ParseEvent::Underline(text) => self.write(&self.theme.underline.paint(text))?,
            ParseEvent::Link { text, url } => {
                self.write(&format_hyperlink(
                    url,
                    &self.theme.link.paint(text),
                    Some(url),
                ))?;
            }
            ParseEvent::Image { alt, .. } => self.write(&format!("[🖼 {}]", alt))?,
            ParseEvent::Footnote(text) => self.write(text)?,
            ParseEvent::Prompt(text) => self.write(text)?,

            // --- Heading ---
            ParseEvent::Heading { level, content } => {
                let styled = match level {
                    1 => self.theme.h1.paint(content),
                    2 => self.theme.h2.paint(content),
                    3 => self.theme.h3.paint(content),
                    4 => self.theme.h4.paint(content),
                    5 => self.theme.h5.paint(content),
                    _ => self.theme.h6.paint(content),
                };
                self.writeln(&styled)?;
            }

            // --- Code block ---
            ParseEvent::CodeBlockStart { language, .. } => {
                self.current_language = language.clone();
            }
            ParseEvent::CodeBlockLine(line) => {
                let highlighted =
                    Highlighter::global().highlight_line(line, self.current_language.as_deref());
                let margin = self.left_margin();
                self.writeln(&format!("{margin}{highlighted}"))?;
            }
            ParseEvent::CodeBlockEnd => {
                self.current_language = None;
            }

            // --- List ---
            ParseEvent::ListItem {
                indent,
                bullet,
                content,
            } => {
                let margin = self.left_margin();
                let pad = "  ".repeat(*indent);
                let ordered = bullet.is_ordered();
                let marker = match bullet {
                    streamdown_parser::ListBullet::Dash => self.theme.bullet.paint("-"),
                    streamdown_parser::ListBullet::Asterisk => self.theme.bullet.paint("*"),
                    streamdown_parser::ListBullet::Plus => self.theme.bullet.paint("+"),
                    streamdown_parser::ListBullet::PlusExpand => self.theme.bullet.paint("+"),
                    streamdown_parser::ListBullet::Ordered(_) => {
                        let n = self.list_state.next_number(*indent, ordered);
                        self.theme.list_number.paint(&format!("{}.", n))
                    }
                };
                let rendered_content = self.render_inline(content);
                self.writeln(&format!("{margin}{pad}{marker} {rendered_content}"))?;
            }
            ParseEvent::ListEnd => {
                self.list_state.mark_pending_reset();
            }

            // --- Table ---
            ParseEvent::TableHeader(cols) | ParseEvent::TableRow(cols) => {
                self.table_rows.push(cols.clone());
            }
            ParseEvent::TableSeparator => {}
            ParseEvent::TableEnd => {
                self.flush_table()?;
            }

            // --- Blockquote ---
            ParseEvent::BlockquoteStart { depth } => {
                self.in_blockquote = true;
                self.blockquote_depth = *depth;
            }
            ParseEvent::BlockquoteLine(text) => {
                let margin = self.left_margin();
                let rendered = self.theme.blockquote_text.paint(text);
                self.writeln(&format!("{margin}{rendered}"))?;
            }
            ParseEvent::BlockquoteEnd => {
                self.in_blockquote = false;
                self.blockquote_depth = 0;
            }

            // --- Think block ---
            ParseEvent::ThinkBlockStart => {
                self.writeln(&self.theme.think_border.paint("┌─ thinking ─"))?;
                self.in_blockquote = true;
                self.blockquote_depth = 1;
            }
            ParseEvent::ThinkBlockLine(text) => {
                let border = self.theme.think_border.paint("│");
                let content = self.theme.think_text.paint(text);
                self.writeln(&format!("{border} {content}"))?;
            }
            ParseEvent::ThinkBlockEnd => {
                self.writeln(&self.theme.think_border.paint("└"))?;
                self.in_blockquote = false;
                self.blockquote_depth = 0;
            }

            // --- HR ---
            ParseEvent::HorizontalRule => {
                let rule = "─".repeat(self.width.min(80));
                self.writeln(&self.theme.hr.paint(&rule))?;
            }

            // --- Whitespace ---
            ParseEvent::EmptyLine | ParseEvent::Newline => {
                self.writeln("")?;
            }

            ParseEvent::InlineElements(elements) => {
                let rendered = self.render_inline_elements(elements);
                self.write(&rendered)?;
            }
        }

        self.writer.flush()
    }

    /// Write raw content (bypasses rendering logic).
    pub fn write_raw(&mut self, s: &str) -> io::Result<()> {
        write!(self.writer, "{}", s)?;
        self.writer.flush()
    }

    fn write(&mut self, s: &str) -> io::Result<()> {
        write!(self.writer, "{}", s)
    }

    fn writeln(&mut self, s: &str) -> io::Result<()> {
        writeln!(self.writer, "{}", s)
    }

    fn left_margin(&self) -> String {
        if self.in_blockquote {
            let border = self.theme.blockquote_border.paint("│");
            format!("{} ", border).repeat(self.blockquote_depth)
        } else {
            String::new()
        }
    }

    fn render_inline(&self, text: &str) -> String {
        use streamdown_parser::inline::InlineParser;
        let elements = InlineParser::new().parse(text);
        self.render_inline_elements(&elements)
    }

    fn render_inline_elements(
        &self,
        elements: &[streamdown_parser::inline::InlineElement],
    ) -> String {
        use streamdown_parser::inline::InlineElement;

        let mut out = String::new();
        for el in elements {
            match el {
                InlineElement::Text(t) => out.push_str(&linkify_issue_refs(t, &self.theme.link)),
                InlineElement::Bold(t) => out.push_str(&self.theme.bold.paint(t)),
                InlineElement::Italic(t) => out.push_str(&self.theme.italic.paint(t)),
                InlineElement::BoldItalic(t) => out.push_str(&self.theme.bold_italic.paint(t)),
                InlineElement::Code(t) => out.push_str(&self.theme.code_inline.paint(t)),
                InlineElement::Strikeout(t) => out.push_str(&self.theme.strikethrough.paint(t)),
                InlineElement::Underline(t) => out.push_str(&self.theme.underline.paint(t)),
                InlineElement::Link { text, url } => {
                    out.push_str(&format_hyperlink(url, &self.theme.link.paint(text), None));
                }
                InlineElement::Image { alt, .. } => {
                    out.push_str(&format!("[🖼 {}]", alt));
                }
                InlineElement::Footnote(t) => out.push_str(t),
            }
        }
        out
    }

    fn flush_table(&mut self) -> io::Result<()> {
        if self.table_rows.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.table_rows);
        let lines = table::render_table(&self.theme, &rows, self.width);
        for line in lines {
            self.writeln(&line)?;
        }
        Ok(())
    }
}
