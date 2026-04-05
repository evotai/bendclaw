use pulldown_cmark::CodeBlockKind;
use pulldown_cmark::Event;
use pulldown_cmark::HeadingLevel;
use pulldown_cmark::Options;
use pulldown_cmark::Parser;
use pulldown_cmark::Tag;
use pulldown_cmark::TagEnd;
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
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Cell;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Row;
use ratatui::widgets::Table;
use ratatui::widgets::Wrap;
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::tui::state::matching_command_hints;
use crate::tui::state::PopupState;
use crate::tui::state::SessionScope;
use crate::tui::state::TranscriptBlock;
use crate::tui::state::TuiState;

const PREVIEW_HEIGHT: u16 = 8;

pub fn render(frame: &mut Frame, state: &TuiState) {
    let mut constraints = Vec::new();
    let popup = popup_height(state);
    if popup > 0 {
        constraints.push(Constraint::Length(popup));
    }
    let preview = preview_height(state);
    if preview > 0 {
        constraints.push(Constraint::Length(preview));
    }
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));
    constraints.push(Constraint::Length(1));

    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(frame.area());

    let mut index = 0;
    if popup > 0 {
        if let Some(popup) = &state.popup {
            render_popup(frame, areas[index], popup, state);
        }
        index += 1;
    }
    if preview > 0 {
        render_preview(frame, areas[index], state);
        index += 1;
    }

    render_status(frame, areas[index], state);
    render_input(frame, areas[index + 1], state);
    render_footer(frame, areas[index + 2], state);
}

pub fn desired_inline_height(state: &TuiState) -> u16 {
    3 + popup_height(state) + preview_height(state)
}

pub fn welcome_block(state: &TuiState) -> TranscriptBlock {
    TranscriptBlock::new(vec![
        Line::from(Span::styled(
            "Bendclaw",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("Model: {}", state.model.label()),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            "Type a prompt or / for commands",
            Style::default().fg(Color::Gray),
        )),
        Line::from(Span::styled(
            "Tab or → complete  ·  Esc stop  ·  Ctrl+C quit",
            Style::default().fg(Color::DarkGray),
        )),
    ])
}

pub fn user_block(text: &str) -> TranscriptBlock {
    TranscriptBlock::new(vec![Line::from(vec![
        Span::styled("> ", Style::default().fg(Color::Yellow)),
        Span::styled(text.to_string(), Style::default().fg(Color::White)),
    ])])
}

pub fn assistant_block(text: &str) -> TranscriptBlock {
    TranscriptBlock::new(render_markdown(text))
}

pub fn log_block(text: impl Into<String>) -> TranscriptBlock {
    TranscriptBlock::new(vec![Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::Gray),
    ))])
}

pub fn divider_block() -> TranscriptBlock {
    TranscriptBlock::new(vec![Line::from(Span::styled(
        "─".repeat(48),
        Style::default().fg(Color::DarkGray),
    ))])
}

pub fn error_block(text: impl Into<String>) -> TranscriptBlock {
    TranscriptBlock::new(vec![Line::from(Span::styled(
        text.into(),
        Style::default().fg(Color::Red),
    ))])
}

pub fn tool_call_block(title: &str, lines: &[String]) -> TranscriptBlock {
    let mut rendered = vec![tool_title_line(title, false, false)];
    rendered.extend(
        lines
            .iter()
            .map(|line| tool_detail_line(line, Style::default().fg(Color::Gray))),
    );
    TranscriptBlock::new(rendered)
}

pub fn tool_result_block(title: &str, lines: &[String], ok: bool) -> TranscriptBlock {
    let mut rendered = vec![tool_title_line(title, true, ok)];
    rendered.extend(lines.iter().map(|line| {
        tool_detail_line(
            line,
            if ok {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Red)
            },
        )
    }));
    TranscriptBlock::new(rendered)
}

fn popup_height(state: &TuiState) -> u16 {
    match state.popup {
        Some(PopupState::Model { .. }) => 12,
        Some(PopupState::Session { .. }) => 14,
        None => 0,
    }
}

fn preview_height(state: &TuiState) -> u16 {
    if state.streaming_assistant.trim().is_empty() {
        0
    } else {
        PREVIEW_HEIGHT
    }
}

fn render_preview(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .title(" Assistant ")
        .borders(Borders::TOP)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut lines = render_markdown(&state.streaming_assistant);
    let max = inner.height as usize;
    if lines.len() > max {
        lines = lines.split_off(lines.len() - max);
    }

    let paragraph = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .scroll((0, 0));
    frame.render_widget(paragraph, inner);
}

fn render_status(frame: &mut Frame, area: Rect, state: &TuiState) {
    let parts = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(32)])
        .split(area);

    let left = if state.loading {
        format!(
            "{} {}  {}",
            spinner_frame(state.spinner_index),
            state
                .status_message
                .clone()
                .unwrap_or_else(|| "Streaming...".into()),
            request_elapsed(state)
        )
    } else {
        String::new()
    };

    let right = state.model.label();
    frame.render_widget(
        Paragraph::new(left).style(Style::default().fg(if state.loading {
            Color::Yellow
        } else {
            Color::DarkGray
        })),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(right)
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::DarkGray)),
        parts[1],
    );
}

fn render_input(frame: &mut Frame, area: Rect, state: &TuiState) {
    let mut spans = vec![Span::styled("> ", Style::default().fg(Color::Yellow))];
    if state.input.is_empty() {
        spans.push(Span::styled(
            "Type a prompt or / for commands",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::raw(state.input.clone()));
    }
    if let Some(suffix) = command_completion_suffix(&state.input) {
        spans.push(Span::styled(suffix, Style::default().fg(Color::DarkGray)));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let cursor_offset =
        input_display_width(&state.input).min(area.width.saturating_sub(3) as usize) as u16;
    frame.set_cursor_position((area.x + 2 + cursor_offset, area.y));
}

fn render_footer(frame: &mut Frame, area: Rect, state: &TuiState) {
    let parts = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(1), Constraint::Length(48)])
        .split(area);

    let left = if state.popup.is_none() && state.input.starts_with('/') {
        command_hint_text(&state.input)
    } else {
        session_text(state)
    };

    frame.render_widget(
        Paragraph::new(left).style(Style::default().fg(Color::DarkGray)),
        parts[0],
    );
    frame.render_widget(
        Paragraph::new(display_path(&state.cwd))
            .alignment(Alignment::Right)
            .style(Style::default().fg(Color::DarkGray)),
        parts[1],
    );
}

fn render_popup(frame: &mut Frame, area: Rect, popup: &PopupState, state: &TuiState) {
    match popup {
        PopupState::Model {
            options,
            selected,
            filter,
        } => render_model_popup(frame, area, options, *selected, filter),
        PopupState::Session {
            options,
            selected,
            filter,
            scope,
        } => render_session_popup(frame, area, options, *selected, filter, *scope, &state.cwd),
    }
}

fn render_model_popup(
    frame: &mut Frame,
    area: Rect,
    options: &[crate::tui::state::ModelOption],
    selected: usize,
    filter: &str,
) {
    let block = Block::default()
        .title(" Model ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));
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

    render_filter_line(frame, parts[0], "Filter models...", filter);

    let filtered = filtered_model_entries(options, filter);
    let rows = filtered
        .iter()
        .enumerate()
        .map(|(index, option)| {
            let style = if index == selected {
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            let marker = if index == selected { "●" } else { " " };
            Row::new(vec![
                Cell::from(Span::styled(marker, Style::default().fg(Color::Yellow))),
                Cell::from(Span::styled(option.model.clone(), style)),
                Cell::from(Span::styled(
                    format!("({})", option.provider),
                    Style::default().fg(Color::Cyan),
                )),
            ])
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Table::new(rows, [
            Constraint::Length(2),
            Constraint::Min(20),
            Constraint::Length(18),
        ]),
        parts[1],
    );
    frame.render_widget(
        Paragraph::new("↑↓ navigate · Enter select · Esc cancel")
            .style(Style::default().fg(Color::DarkGray)),
        parts[2],
    );
}

fn render_session_popup(
    frame: &mut Frame,
    area: Rect,
    options: &[crate::storage::model::SessionMeta],
    selected: usize,
    filter: &str,
    scope: SessionScope,
    cwd: &str,
) {
    let block = Block::default()
        .title(" Sessions ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Gray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(inner);

    let tabs = Line::from(vec![
        Span::styled(
            if scope == SessionScope::CurrentFolder {
                "◉ Current Folder"
            } else {
                "○ Current Folder"
            },
            Style::default().fg(if scope == SessionScope::CurrentFolder {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
        Span::styled(" | ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            if scope == SessionScope::All {
                "◉ All"
            } else {
                "○ All"
            },
            Style::default().fg(if scope == SessionScope::All {
                Color::Yellow
            } else {
                Color::DarkGray
            }),
        ),
    ]);
    frame.render_widget(Paragraph::new(tabs).alignment(Alignment::Right), parts[0]);
    render_filter_line(frame, parts[1], "Type to filter sessions...", filter);

    let filtered = filtered_session_entries(options, filter, cwd, scope);
    let header = Row::new(vec![
        Cell::from("Modified"),
        Cell::from("Turns"),
        Cell::from("Model"),
        Cell::from("Title"),
    ])
    .style(Style::default().fg(Color::DarkGray));

    let rows = filtered
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let title = summarize_title(
                &session
                    .title
                    .clone()
                    .unwrap_or_else(|| "Untitled session".into()),
            );
            let style = if index == selected {
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Rgb(50, 50, 50))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };

            Row::new(vec![
                Cell::from(relative_time(&session.updated_at)),
                Cell::from(session.turns.to_string()),
                Cell::from(session.model.clone()),
                Cell::from(title),
            ])
            .style(style)
        })
        .collect::<Vec<_>>();

    frame.render_widget(
        Table::new(rows, [
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(20),
            Constraint::Min(20),
        ])
        .header(header)
        .column_spacing(1),
        parts[2],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "↑↓ navigate · Enter select · ←→ scope · Esc close   {} sessions",
            filtered.len()
        ))
        .style(Style::default().fg(Color::DarkGray)),
        parts[3],
    );
}

fn render_filter_line(frame: &mut Frame, area: Rect, placeholder: &str, filter: &str) {
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

fn filtered_model_entries(
    options: &[crate::tui::state::ModelOption],
    filter: &str,
) -> Vec<crate::tui::state::ModelOption> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .filter(|option| {
            filter.is_empty()
                || option.model.to_lowercase().contains(&filter)
                || option.label().to_lowercase().contains(&filter)
        })
        .cloned()
        .collect()
}

fn filtered_session_entries(
    options: &[crate::storage::model::SessionMeta],
    filter: &str,
    cwd: &str,
    scope: SessionScope,
) -> Vec<crate::storage::model::SessionMeta> {
    let filter = filter.trim().to_lowercase();
    options
        .iter()
        .filter(|session| scope == SessionScope::All || session.cwd == cwd)
        .filter(|session| {
            if filter.is_empty() {
                return true;
            }

            session.session_id.to_lowercase().contains(&filter)
                || session.model.to_lowercase().contains(&filter)
                || session
                    .title
                    .as_ref()
                    .map(|title| title.to_lowercase().contains(&filter))
                    .unwrap_or(false)
        })
        .cloned()
        .collect()
}

fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut current = Vec::new();
    let mut heading: Option<HeadingLevel> = None;
    let mut list_stack: Vec<Option<u64>> = Vec::new();
    let mut inline_style = Style::default().fg(Color::White);
    let mut in_code_block = false;

    for event in Parser::new_ext(text, Options::all()) {
        match event {
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => push_line(&mut lines, &mut current),
            Event::Start(Tag::Heading { level, .. }) => {
                push_line(&mut lines, &mut current);
                heading = Some(level);
            }
            Event::End(TagEnd::Heading(_)) => {
                push_line(&mut lines, &mut current);
                lines.push(Line::default());
                heading = None;
            }
            Event::Start(Tag::List(start)) => {
                list_stack.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
                lines.push(Line::default());
            }
            Event::Start(Tag::Item) => {
                push_line(&mut lines, &mut current);
                let prefix = match list_stack.last_mut() {
                    Some(Some(index)) => {
                        let value = *index;
                        *index += 1;
                        format!("{value}. ")
                    }
                    _ => "• ".into(),
                };
                current.push(Span::styled(prefix, Style::default().fg(Color::Yellow)));
            }
            Event::End(TagEnd::Item) => push_line(&mut lines, &mut current),
            Event::Start(Tag::CodeBlock(kind)) => {
                push_line(&mut lines, &mut current);
                in_code_block = true;
                if let CodeBlockKind::Fenced(lang) = kind {
                    lines.push(Line::from(Span::styled(
                        format!("```{lang}"),
                        Style::default().fg(Color::Cyan),
                    )));
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                lines.push(Line::from(Span::styled(
                    "```",
                    Style::default().fg(Color::Cyan),
                )));
            }
            Event::Start(Tag::Strong) => {
                inline_style = inline_style.add_modifier(Modifier::BOLD);
            }
            Event::End(TagEnd::Strong) => {
                inline_style = Style::default().fg(Color::White);
            }
            Event::Start(Tag::Emphasis) => {
                inline_style = inline_style.add_modifier(Modifier::ITALIC);
            }
            Event::End(TagEnd::Emphasis) => {
                inline_style = Style::default().fg(Color::White);
            }
            Event::Code(code) => current.push(Span::styled(
                code.to_string(),
                Style::default()
                    .fg(Color::Yellow)
                    .bg(Color::Rgb(48, 48, 48)),
            )),
            Event::Text(value) => {
                if in_code_block {
                    for line in value.lines() {
                        lines.push(Line::from(Span::styled(
                            line.to_string(),
                            Style::default()
                                .fg(Color::Rgb(210, 240, 255))
                                .bg(Color::Rgb(42, 42, 42)),
                        )));
                    }
                } else {
                    current.push(Span::styled(
                        value.to_string(),
                        heading_style(heading).patch(inline_style),
                    ));
                }
            }
            Event::SoftBreak => push_line(&mut lines, &mut current),
            Event::HardBreak => {
                push_line(&mut lines, &mut current);
                lines.push(Line::default());
            }
            _ => {}
        }
    }

    push_line(&mut lines, &mut current);
    lines
}

fn tool_title_line(title: &str, is_result: bool, ok: bool) -> Line<'static> {
    let (badge, rest) = split_tool_title(title);
    let (fg, bg) = if is_result {
        if ok {
            (Color::Black, Color::Rgb(133, 220, 140))
        } else {
            (Color::White, Color::Rgb(157, 57, 57))
        }
    } else {
        (Color::Black, Color::Rgb(245, 197, 66))
    };

    let mut spans = vec![Span::styled(
        format!("[{}]", badge),
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD),
    )];
    if !rest.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(rest, Style::default().fg(Color::Gray)));
    }
    Line::from(spans)
}

fn tool_detail_line(text: &str, style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled("  ", Style::default().fg(Color::DarkGray)),
        Span::styled(text.to_string(), style),
    ])
}

fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
}

fn heading_style(level: Option<HeadingLevel>) -> Style {
    match level {
        Some(HeadingLevel::H1) => Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
        Some(HeadingLevel::H2) => Style::default()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
        Some(HeadingLevel::H3) => Style::default()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::White),
    }
}

fn push_line(lines: &mut Vec<Line<'static>>, current: &mut Vec<Span<'static>>) {
    if !current.is_empty() {
        lines.push(Line::from(std::mem::take(current)));
    }
}

fn spinner_frame(index: usize) -> &'static str {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    FRAMES[index % FRAMES.len()]
}

fn request_elapsed(state: &TuiState) -> String {
    match state.request_started_at {
        Some(started_at) => format_elapsed(started_at.elapsed().as_secs()),
        None => "0s".into(),
    }
}

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
    }
}

fn session_text(state: &TuiState) -> String {
    match &state.session_id {
        Some(session_id) => format!("session {}", short_id(session_id)),
        None => "new session".into(),
    }
}

fn short_id(value: &str) -> String {
    value.chars().take(8).collect()
}

fn input_display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

fn command_completion_suffix(input: &str) -> Option<String> {
    let hint = matching_command_hints(input).first().copied()?;
    if hint.command == input {
        return None;
    }
    hint.command
        .strip_prefix(input)
        .map(|suffix| suffix.to_string())
}

fn command_hint_text(input: &str) -> String {
    let matches = matching_command_hints(input);
    if matches.is_empty() {
        return "no matching command".into();
    }

    matches
        .into_iter()
        .take(3)
        .map(|hint| format!("{} {}", hint.command, hint.summary))
        .collect::<Vec<_>>()
        .join("   ")
}

fn display_path(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

fn relative_time(value: &str) -> String {
    match chrono::DateTime::parse_from_rfc3339(value) {
        Ok(datetime) => {
            let duration =
                chrono::Utc::now().signed_duration_since(datetime.with_timezone(&chrono::Utc));
            if duration.num_minutes() <= 0 {
                "just now".into()
            } else if duration.num_hours() <= 0 {
                format!("{}m ago", duration.num_minutes())
            } else if duration.num_days() <= 0 {
                format!("{}h ago", duration.num_hours())
            } else {
                format!("{}d ago", duration.num_days())
            }
        }
        Err(_) => value.into(),
    }
}

fn summarize_title(value: &str) -> String {
    let mut title: String = value.chars().take(48).collect();
    if value.chars().count() > 48 {
        title.push_str("...");
    }
    title
}
