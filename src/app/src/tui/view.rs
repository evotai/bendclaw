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
use crate::tui::state::MessageItem;
use crate::tui::state::PopupState;
use crate::tui::state::SessionScope;
use crate::tui::state::TuiState;

pub fn render(frame: &mut Frame, state: &TuiState) {
    let popup_height = popup_height(state);
    let areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(popup_height),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

    render_messages(frame, areas[0], state);

    if popup_height > 0 {
        if let Some(popup) = &state.popup {
            render_popup(frame, areas[1], popup, state);
        }
    }

    render_status(frame, areas[2], state);
    render_input(frame, areas[3], state);
    render_footer(frame, areas[4], state);
}

fn popup_height(state: &TuiState) -> u16 {
    match state.popup {
        Some(PopupState::Model { .. }) => 14,
        Some(PopupState::Session { .. }) => 16,
        None => 0,
    }
}

fn render_messages(frame: &mut Frame, area: Rect, state: &TuiState) {
    if state.messages.is_empty() {
        render_welcome(frame, area, state);
        return;
    }

    let lines = build_message_lines(state, area.height as usize);
    let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_status(frame: &mut Frame, area: Rect, state: &TuiState) {
    let (text, alignment, color) = if state.loading {
        (
            format!(
                "{} Streaming...  {}   (Press ESC to stop)",
                spinner_frame(state.spinner_index),
                request_elapsed(state)
            ),
            Alignment::Left,
            Color::Yellow,
        )
    } else {
        (state.model.label(), Alignment::Right, Color::DarkGray)
    };

    let paragraph = Paragraph::new(text)
        .alignment(alignment)
        .style(Style::default().fg(color));
    frame.render_widget(paragraph, area);
}

fn render_input(frame: &mut Frame, area: Rect, state: &TuiState) {
    let block = Block::default()
        .borders(Borders::TOP | Borders::BOTTOM)
        .border_style(Style::default().fg(Color::Gray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

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
    let input = Paragraph::new(Line::from(spans));
    frame.render_widget(input, inner);
    let cursor_offset =
        input_display_width(&state.input).min(inner.width.saturating_sub(1) as usize) as u16;
    frame.set_cursor_position((inner.x + 2 + cursor_offset, inner.y));
}

fn render_footer(frame: &mut Frame, area: Rect, state: &TuiState) {
    let elapsed = format_elapsed(state.session_started_at.elapsed().as_secs());
    let cwd = display_path(&state.cwd);
    let footer_text = if state.popup.is_none() && state.input.starts_with('/') {
        format!("{}   {}", command_hint_text(&state.input), cwd)
    } else {
        format!("[⏱ {elapsed}]  ? for help   {cwd}")
    };
    let footer = Paragraph::new(footer_text).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, area);
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
        .title(" Select Model For This Session ")
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
    let mut rows = Vec::new();
    for (index, option) in filtered.iter().enumerate() {
        let style = if index == selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };

        let marker = if index == selected { "●" } else { " " };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(marker, Style::default().fg(Color::Yellow))),
            Cell::from(Span::styled(option.model.clone(), style)),
            Cell::from(Span::styled(
                format!("({})", option.provider),
                Style::default().fg(Color::Cyan),
            )),
        ]));
    }

    let table = Table::new(rows, [
        Constraint::Length(2),
        Constraint::Min(20),
        Constraint::Length(18),
    ]);
    frame.render_widget(table, parts[1]);

    let footer = Paragraph::new("↑↓ navigate · Enter select · Esc cancel")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, parts[2]);
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
        Span::raw(""),
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
    let tabs_widget = Paragraph::new(tabs).alignment(ratatui::layout::Alignment::Right);
    frame.render_widget(tabs_widget, parts[0]);
    render_filter_line(frame, parts[1], "Type to filter sessions...", filter);

    let header = Row::new(vec![
        Cell::from("Modified"),
        Cell::from("Turns"),
        Cell::from("Model"),
        Cell::from("Title"),
    ])
    .style(Style::default().fg(Color::DarkGray));
    let filtered = filtered_session_entries(options, filter, cwd, scope);
    let mut rows = Vec::new();
    for (index, session) in filtered.iter().enumerate() {
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
        rows.push(
            Row::new(vec![
                Cell::from(relative_time(&session.updated_at)),
                Cell::from(session.turns.to_string()),
                Cell::from(session.model.clone()),
                Cell::from(title),
            ])
            .style(style),
        );
    }

    let table = Table::new(rows, [
        Constraint::Length(12),
        Constraint::Length(8),
        Constraint::Length(20),
        Constraint::Min(20),
    ])
    .header(header)
    .column_spacing(1);
    frame.render_widget(table, parts[2]);

    let footer = Paragraph::new(format!(
        "↑↓ navigate · Enter select · ←→ scope · Esc close   {} sessions",
        filtered.len()
    ))
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, parts[3]);
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
    let paragraph = Paragraph::new(Line::from(vec![
        Span::styled("█", Style::default().fg(Color::White)),
        Span::raw(" "),
        Span::styled(text.to_string(), style),
    ]));
    frame.render_widget(paragraph, area);
}

fn build_message_lines(state: &TuiState, max_lines: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for item in &state.messages {
        match item {
            MessageItem::Log(text) => {
                lines.push(Line::from(Span::styled(
                    text.clone(),
                    Style::default().fg(Color::Gray),
                )));
            }
            MessageItem::User(text) => {
                lines.push(Line::from(Span::styled(
                    text.clone(),
                    Style::default().fg(Color::White),
                )));
            }
            MessageItem::Assistant(text) => {
                lines.extend(render_markdown(text));
            }
            MessageItem::ToolCall {
                title,
                lines: detail,
            } => {
                lines.push(tool_title_line(title, false, false));
                lines.extend(
                    detail
                        .iter()
                        .map(|line| tool_detail_line(line, Color::Gray)),
                );
            }
            MessageItem::ToolResult {
                title,
                lines: detail,
                ok,
            } => {
                lines.push(tool_title_line(title, true, *ok));
                lines.extend(detail.iter().map(|line| {
                    tool_detail_line(line, if *ok { Color::Green } else { Color::Red })
                }));
            }
            MessageItem::Error(text) => {
                lines.push(Line::from(Span::styled(
                    text.clone(),
                    Style::default().fg(Color::Red),
                )));
            }
        }
        lines.push(Line::default());
    }

    if let Some(text) = &state.streaming_assistant {
        lines.extend(render_markdown(text));
    }

    if lines.len() > max_lines {
        let start = lines.len() - max_lines;
        lines = lines.split_off(start);
    }

    if lines.len() < max_lines {
        let mut padded = Vec::with_capacity(max_lines);
        for _ in 0..(max_lines - lines.len()) {
            padded.push(Line::default());
        }
        padded.extend(lines);
        return padded;
    }

    lines
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

fn format_elapsed(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else {
        format!("{}m {}s", seconds / 60, seconds % 60)
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

fn display_path(path: &str) -> String {
    match std::env::var("HOME") {
        Ok(home) if path.starts_with(&home) => format!("~{}", &path[home.len()..]),
        _ => path.to_string(),
    }
}

fn render_welcome(frame: &mut Frame, area: Rect, state: &TuiState) {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),
            Constraint::Length(12),
            Constraint::Min(1),
        ])
        .split(area);
    let center = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(6), Constraint::Length(6)])
        .split(vertical[1]);

    let logo = Paragraph::new(Text::from(vec![
        Line::from("██████╗ ███████╗███╗   ██╗██████╗  ██████╗██╗      █████╗ ██╗    ██╗"),
        Line::from("██╔══██╗██╔════╝████╗  ██║██╔══██╗██╔════╝██║     ██╔══██╗██║    ██║"),
        Line::from("██████╔╝█████╗  ██╔██╗ ██║██║  ██║██║     ██║     ███████║██║ █╗ ██║"),
        Line::from("██╔══██╗██╔══╝  ██║╚██╗██║██║  ██║██║     ██║     ██╔══██║██║███╗██║"),
        Line::from("██████╔╝███████╗██║ ╚████║██████╔╝╚██████╗███████╗██║  ██║╚███╔███╔╝"),
        Line::from("╚═════╝ ╚══════╝╚═╝  ╚═══╝╚═════╝  ╚═════╝╚══════╝╚═╝  ╚═╝ ╚══╝╚══╝ "),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().fg(Color::White));

    let meta = Paragraph::new(Text::from(vec![
        Line::from(Span::styled(
            env!("CARGO_PKG_VERSION"),
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "TIP: Use /sessions to resume a conversation",
            Style::default().fg(Color::Gray),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Tab or → to complete commands  ·  Esc to stop  ·  Ctrl+C to quit",
            Style::default().fg(Color::DarkGray),
        )),
        Line::from(Span::styled(
            format!("Model: {}", state.model.label()),
            Style::default().fg(Color::DarkGray),
        )),
    ]))
    .alignment(Alignment::Center);

    frame.render_widget(logo, center[0]);
    frame.render_widget(meta, center[1]);
}

fn input_display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
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

fn tool_detail_line(text: &str, color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("  ", Style::default().fg(Color::DarkGray)),
        Span::styled(text.to_string(), Style::default().fg(color)),
    ])
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

fn split_tool_title(title: &str) -> (String, String) {
    let mut parts = title.split_whitespace();
    let badge = parts.next().unwrap_or("TOOL").to_uppercase();
    let rest = parts.collect::<Vec<_>>().join(" ");
    (badge, rest)
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
