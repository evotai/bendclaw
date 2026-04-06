//! Renders ParseEvent into ANSI terminal output.

use std::io::Write;
use std::io::{self};

use streamdown_parser::ParseEvent;

use super::highlight::Highlighter;
use super::theme::Theme;

pub struct Renderer<W: Write> {
    writer: W,
    width: usize,
    theme: Theme,
    highlighter: Highlighter,
    current_language: Option<String>,
    in_blockquote: bool,
    blockquote_depth: usize,
    table_rows: Vec<Vec<String>>,
}

impl<W: Write> Renderer<W> {
    pub fn new(writer: W, width: usize) -> Self {
        Self {
            writer,
            width,
            theme: Theme::default(),
            highlighter: Highlighter::default(),
            current_language: None,
            in_blockquote: false,
            blockquote_depth: 0,
            table_rows: Vec::new(),
        }
    }

    pub fn render_event(&mut self, event: &ParseEvent) -> io::Result<()> {
        match event {
            // --- Inline ---
            ParseEvent::Text(text) => self.write(&self.theme.text.paint(text))?,
            ParseEvent::Bold(text) => self.write(&self.theme.bold.paint(text))?,
            ParseEvent::Italic(text) => self.write(&self.theme.italic.paint(text))?,
            ParseEvent::BoldItalic(text) => self.write(&self.theme.bold_italic.paint(text))?,
            ParseEvent::InlineCode(text) => self.write(&self.theme.code_inline.paint(text))?,
            ParseEvent::Strikeout(text) => self.write(&self.theme.strikethrough.paint(text))?,
            ParseEvent::Underline(text) => self.write(&self.theme.underline.paint(text))?,
            ParseEvent::Link { text, url } => {
                // OSC 8 hyperlink + fallback
                let styled = format!(
                    "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\ ({})",
                    url,
                    self.theme.link.paint(text),
                    url
                );
                self.write(&styled)?;
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
                let highlighted = self
                    .highlighter
                    .highlight_line(line, self.current_language.as_deref());
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
                let marker = match bullet {
                    streamdown_parser::ListBullet::Dash => self.theme.bullet.paint("-"),
                    streamdown_parser::ListBullet::Asterisk => self.theme.bullet.paint("*"),
                    streamdown_parser::ListBullet::Plus => self.theme.bullet.paint("+"),
                    streamdown_parser::ListBullet::PlusExpand => self.theme.bullet.paint("+"),
                    streamdown_parser::ListBullet::Ordered(n) => {
                        self.theme.list_number.paint(&format!("{}.", n))
                    }
                };
                let rendered_content = self.render_inline(content);
                self.writeln(&format!("{margin}{pad}{marker} {rendered_content}"))?;
            }
            ParseEvent::ListEnd => {}

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

    // --- helpers ---

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

    /// Render inline markdown formatting within content strings (e.g. list item content).
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
                InlineElement::Text(t) => out.push_str(t),
                InlineElement::Bold(t) => out.push_str(&self.theme.bold.paint(t)),
                InlineElement::Italic(t) => out.push_str(&self.theme.italic.paint(t)),
                InlineElement::BoldItalic(t) => out.push_str(&self.theme.bold_italic.paint(t)),
                InlineElement::Code(t) => out.push_str(&self.theme.code_inline.paint(t)),
                InlineElement::Strikeout(t) => out.push_str(&self.theme.strikethrough.paint(t)),
                InlineElement::Underline(t) => out.push_str(&self.theme.underline.paint(t)),
                InlineElement::Link { text, url } => {
                    out.push_str(&format!(
                        "\x1b]8;;{}\x1b\\{}\x1b]8;;\x1b\\",
                        url,
                        self.theme.link.paint(text)
                    ));
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

        // Calculate column widths
        let col_count = rows.iter().map(|r| r.len()).max().unwrap_or(0);
        let mut widths = vec![0usize; col_count];
        for row in &rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(cell.len());
            }
        }

        let border_char = self.theme.table_border.paint("│");
        let h_border = self.theme.table_border.paint("─");

        for (row_idx, row) in rows.iter().enumerate() {
            let mut line = format!("{} ", border_char);
            for (i, cell) in row.iter().enumerate() {
                let w = widths.get(i).copied().unwrap_or(0);
                let styled = if row_idx == 0 {
                    self.theme.table_header.paint(cell)
                } else {
                    cell.to_string()
                };
                let padding = w.saturating_sub(cell.len());
                line.push_str(&styled);
                line.push_str(&" ".repeat(padding));
                line.push_str(&format!(" {} ", border_char));
            }
            self.writeln(&line)?;

            // Separator after header
            if row_idx == 0 {
                let mut sep = format!("{} ", border_char);
                for (i, _) in row.iter().enumerate() {
                    let w = widths.get(i).copied().unwrap_or(0);
                    for _ in 0..w {
                        sep.push_str(&h_border);
                    }
                    sep.push_str(&format!(" {} ", border_char));
                }
                self.writeln(&sep)?;
            }
        }
        Ok(())
    }
}
