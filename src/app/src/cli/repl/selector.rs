use std::time::Duration;

use crossterm::cursor::Hide;
use crossterm::cursor::Show;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Alignment;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Clear as ClearWidget;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Row;
use ratatui::widgets::Table;
use ratatui::Terminal;

use crate::conf::Config;
use crate::error::BendclawError;
use crate::error::Result;

pub const MAX_SELECTOR_ROWS: usize = 12;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

pub struct SelectorOption {
    pub id: String,
    pub primary: String,
    pub secondary: String,
}

struct SelectorPopup<'a> {
    title: &'a str,
    placeholder: &'a str,
    footer: &'a str,
    options: &'a [SelectorOption],
    filtered: &'a [usize],
    filter: &'a str,
    selected: usize,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunControl {
    Cancel,
    Exit,
}

pub enum PromptExit {
    Finished(String, bool), // session_id, exit_requested
    Cancelled(bool),
}

// ---------------------------------------------------------------------------
// Selector
// ---------------------------------------------------------------------------

pub fn run_selector(
    title: &str,
    placeholder: &str,
    footer: &str,
    options: &[SelectorOption],
    selected: Option<usize>,
) -> Result<Option<usize>> {
    let mut terminal = SelectorTerminal::enter()?;
    let mut filter = String::new();
    let mut selected = selected.unwrap_or(0);

    loop {
        let filtered = filtered_selector_indices(options, &filter);
        if filtered.is_empty() {
            selected = 0;
        } else if selected >= filtered.len() {
            selected = filtered.len() - 1;
        }

        let popup = SelectorPopup {
            title,
            placeholder,
            footer,
            options,
            filtered: &filtered,
            filter: &filter,
            selected,
        };

        terminal.draw(|frame| render_selector_popup(frame, &popup))?;

        let event = read()?;
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => return Ok(None),
                KeyCode::Enter => return Ok(filtered.get(selected).copied()),
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => {
                    if !filtered.is_empty() && selected + 1 < filtered.len() {
                        selected += 1;
                    }
                }
                KeyCode::Backspace => {
                    filter.pop();
                    selected = 0;
                }
                KeyCode::Char(ch) => {
                    filter.push(ch);
                    selected = 0;
                }
                _ => {}
            },
            _ => {}
        }
    }
}

fn filtered_selector_indices(options: &[SelectorOption], filter: &str) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .enumerate()
        .filter(|(_, option)| {
            filter.is_empty()
                || option.primary.to_lowercase().contains(&filter)
                || option.secondary.to_lowercase().contains(&filter)
                || option.id.to_lowercase().contains(&filter)
        })
        .map(|(index, _)| index)
        .collect()
}

fn render_selector_popup(frame: &mut ratatui::Frame<'_>, popup: &SelectorPopup<'_>) {
    let area = selector_area(frame.area(), popup.filtered.len());
    frame.render_widget(ClearWidget, area);

    let block = Block::default()
        .title(Line::from(vec![Span::styled(
            format!(" {} ", popup.title),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )]))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    render_selector_filter(frame, parts[0], popup.placeholder, popup.filter);

    if popup.filtered.is_empty() {
        frame.render_widget(
            Paragraph::new("no matches")
                .alignment(Alignment::Left)
                .style(Style::default().fg(Color::DarkGray)),
            parts[1],
        );
    } else {
        let start = selector_scroll_start(popup.selected, popup.filtered.len());
        let rows = popup
            .filtered
            .iter()
            .enumerate()
            .skip(start)
            .take(MAX_SELECTOR_ROWS)
            .map(|(visible_index, option_index)| {
                let option = &popup.options[*option_index];
                let selected_row = visible_index == popup.selected;
                let row_style = if selected_row {
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Rgb(50, 50, 50))
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Gray)
                };
                let marker = if selected_row { "●" } else { " " };
                Row::new(vec![
                    Cell::from(Span::styled(marker, Style::default().fg(Color::Yellow))),
                    Cell::from(option.primary.clone()),
                    Cell::from(option.secondary.clone()),
                ])
                .style(row_style)
            })
            .collect::<Vec<_>>();

        frame.render_widget(
            Table::new(rows, [
                Constraint::Length(2),
                Constraint::Percentage(46),
                Constraint::Min(18),
            ])
            .column_spacing(1),
            parts[1],
        );
    }

    frame.render_widget(
        Paragraph::new(format!("{}   {} items", popup.footer, popup.filtered.len()))
            .style(Style::default().fg(Color::DarkGray)),
        parts[2],
    );
}

fn render_selector_filter(
    frame: &mut ratatui::Frame<'_>,
    area: Rect,
    placeholder: &str,
    filter: &str,
) {
    let text = if filter.is_empty() {
        placeholder
    } else {
        filter
    };
    let style = if filter.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::White)
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("█", Style::default().fg(Color::White)),
            Span::raw(" "),
            Span::styled(text.to_string(), style),
        ])),
        area,
    );
}

fn selector_scroll_start(selected: usize, len: usize) -> usize {
    if len <= MAX_SELECTOR_ROWS {
        0
    } else if selected >= MAX_SELECTOR_ROWS {
        selected + 1 - MAX_SELECTOR_ROWS
    } else {
        0
    }
}

fn selector_area(frame_area: Rect, item_count: usize) -> Rect {
    let rows = item_count.clamp(1, MAX_SELECTOR_ROWS) as u16;
    let width = frame_area
        .width
        .saturating_mul(88)
        .saturating_div(100)
        .max(56);
    let height = (rows + 6).min(frame_area.height.saturating_sub(2)).max(8);
    centered_rect(frame_area, width.min(frame_area.width), height)
}

fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(height),
            Constraint::Fill(1),
        ])
        .split(area);
    let horizontal = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Length(width),
            Constraint::Fill(1),
        ])
        .split(vertical[1]);
    horizontal[1]
}

// ---------------------------------------------------------------------------
// SelectorTerminal
// ---------------------------------------------------------------------------

struct SelectorTerminal {
    terminal: Terminal<CrosstermBackend<std::io::Stdout>>,
}

impl SelectorTerminal {
    fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut stdout = std::io::stdout();
        execute!(stdout, EnterAlternateScreen, Hide)?;
        let backend = CrosstermBackend::new(stdout);
        let terminal = Terminal::new(backend)
            .map_err(|e| BendclawError::Cli(format!("failed to initialize selector: {e}")))?;
        Ok(Self { terminal })
    }

    fn draw<F>(&mut self, render: F) -> Result<()>
    where F: FnOnce(&mut ratatui::Frame<'_>) {
        self.terminal
            .draw(render)
            .map_err(|e| BendclawError::Cli(format!("failed to draw selector: {e}")))?;
        Ok(())
    }
}

impl Drop for SelectorTerminal {
    fn drop(&mut self) {
        let _ = self.terminal.show_cursor();
        let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        let _ = disable_raw_mode();
    }
}

// ---------------------------------------------------------------------------
// Run control
// ---------------------------------------------------------------------------

pub fn wait_for_run_control(
    run_task: &mut tokio::task::JoinHandle<Result<String>>,
    spinner: &std::sync::Mutex<super::spinner::SpinnerState>,
) -> Result<Option<RunControl>> {
    let _guard = RawModeGuard::enter()?;
    loop {
        if run_task.is_finished() {
            if let Ok(mut state) = spinner.lock() {
                state.clear_if_rendered();
            }
            return Ok(None);
        }
        if !poll(Duration::from_millis(80))? {
            if let Ok(mut state) = spinner.lock() {
                if state.is_active() {
                    state.render_frame();
                }
            }
            continue;
        }
        match read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Esc => {
                    if let Ok(mut state) = spinner.lock() {
                        state.clear_if_rendered();
                    }
                    return Ok(Some(RunControl::Cancel));
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if let Ok(mut state) = spinner.lock() {
                        state.clear_if_rendered();
                    }
                    return Ok(Some(RunControl::Exit));
                }
                _ => {}
            },
            _ => {}
        }
    }
}

pub struct RawModeGuard;

impl RawModeGuard {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
    }
}

// ---------------------------------------------------------------------------
// Model/provider helpers
// ---------------------------------------------------------------------------

pub fn available_models(config: &Config) -> Vec<String> {
    let mut models = Vec::new();
    for model in [
        config.anthropic.model.clone(),
        config.openai.model.clone(),
        config.active_llm().model,
    ] {
        if !model.trim().is_empty() && !models.contains(&model) {
            models.push(model);
        }
    }
    models
}

pub fn provider_marker_for_model(config: &Config, model: &str) -> &'static str {
    if config.anthropic.model == model && config.openai.model == model {
        "anthropic/openai"
    } else if config.anthropic.model == model {
        "anthropic"
    } else if config.openai.model == model {
        "openai"
    } else {
        "custom"
    }
}
