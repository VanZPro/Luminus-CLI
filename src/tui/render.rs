use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::agent::AgentStatus;
use crate::app::{App, Role, UiMode};
use crate::tool_activity::ToolStatus;

use super::{
    logo::{self, ASCII_BANNER, TAGLINE},
    theme::Theme,
};

pub(crate) fn draw(frame: &mut Frame<'_>, app: &App, theme: Theme) {
    draw_with_composer(frame, app, theme, "");
}

pub(crate) fn draw_with_composer(frame: &mut Frame<'_>, app: &App, theme: Theme, composer: &str) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background).fg(theme.foreground)),
        area,
    );

    let compact = area.width < 70;
    let wide = area.width >= 100;
    // Keep the footer visible even on small terminals: it is the primary affordance
    // for discovering the command-line composer and current session settings.
    // The header is deliberately bounded so the conversation and composer keep
    // their space when a terminal is resized.
    let header_height = if wide {
        9
    } else if compact {
        3
    } else {
        5
    };
    // Reserve a real bordered composer box instead of rendering the input as a
    // loose line below the status bar. This keeps typed text inside the panel
    // while still leaving conversation space responsive on short terminals.
    //
    // Cap the header against the actual viewport: ratatui cannot give the
    // conversation a negative height when a terminal is shorter than the
    // normal header plus footer reservation.
    let footer_height = area.height.min(5);
    let header_height = header_height.min(area.height.saturating_sub(footer_height));
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(header_height),
            Constraint::Min(0),
            Constraint::Length(footer_height),
        ])
        .split(area);

    render_header(frame, sections[0], theme, compact, wide);
    if sections[1].height > 0 {
        render_conversation(frame, sections[1], app, theme);
    }
    render_status(frame, sections[2], theme, compact, app, composer);

    if app.ui_mode() == UiMode::ModelSelector {
        render_model_selector(frame, area, app, theme);
    }
}

/// Centered popup listing configured models; Up/Down to move, Enter to select,
/// Esc to dismiss.
fn render_model_selector(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let items = &app.model_selector_items;
    let content_height = items.len() as u16 + 2; // borders
    let width = area.width.saturating_sub(4).clamp(20, 60);
    let height = content_height.min(area.height.saturating_sub(2)).max(3);
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);

    frame.render_widget(ratatui::widgets::Clear, popup);

    let lines: Vec<Line<'_>> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            if i == app.model_selector_index {
                Line::from(Span::styled(
                    format!("> {item}"),
                    Style::default()
                        .fg(theme.background)
                        .bg(theme.primary)
                        .add_modifier(Modifier::BOLD),
                ))
            } else {
                Line::from(Span::styled(
                    format!("  {item}"),
                    Style::default().fg(theme.foreground),
                ))
            }
        })
        .collect();

    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SELECT MODEL — ↑↓ move · Enter select · Esc close ")
                .border_style(Style::default().fg(theme.accent))
                .style(Style::default().bg(theme.background)),
        ),
        popup,
    );
}

fn render_status(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: Theme,
    compact: bool,
    app: &App,
    composer: &str,
) {
    let context = app
        .messages
        .iter()
        .map(|m| m.content.split_whitespace().count())
        .sum::<usize>();
    let model = "default";
    let mode = if app.messages.iter().any(|m| m.role == Role::Assistant) {
        "chat"
    } else {
        "ready"
    };
    let safety = "safe";
    let hint = if compact {
        "Enter: send  Ctrl+C: quit"
    } else {
        "Prompt  Enter: send   Ctrl+L: clear   Ctrl+C: quit"
    };
    let composer_text = if composer.is_empty() { "" } else { composer };
    let status_line = Line::from(vec![
        Span::styled(
            format!(" {model} "),
            Style::default()
                .fg(theme.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("│ {mode} │ ctx {context} tok │ {safety}"),
            Style::default().fg(theme.foreground),
        ),
        Span::styled(
            format!("│ {}", if compact { "ready" } else { hint }),
            Style::default().fg(theme.muted),
        ),
    ]);
    let composer_line = Line::from(Span::styled(
        format!(" > {composer_text}"),
        Style::default().fg(if theme.monochrome {
            theme.foreground
        } else {
            theme.accent
        }),
    ));
    let footer = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(2)])
        .split(area);

    let title = if compact {
        " STATUS "
    } else {
        " STATUS  •  COMPOSER "
    };
    frame.render_widget(
        Paragraph::new(status_line).block(
            Block::default()
                .borders(Borders::TOP)
                .title(title)
                .border_style(Style::default().fg(theme.border)),
        ),
        footer[0],
    );

    frame.render_widget(
        Paragraph::new(composer_line)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" PROMPT ")
                    .border_style(Style::default().fg(theme.border)),
            )
            .wrap(Wrap { trim: false }),
        footer[1],
    );
}

fn render_header(frame: &mut Frame<'_>, area: Rect, theme: Theme, compact: bool, wide: bool) {
    let title = logo::title(area.width);
    if wide {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(area);
        let banner = ASCII_BANNER
            .lines()
            .map(|line| Line::from(Span::styled(line, Style::default().fg(theme.primary))))
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(banner).alignment(Alignment::Center).block(
                Block::default()
                    .borders(Borders::BOTTOM)
                    .border_style(Style::default().fg(theme.border)),
            ),
            columns[0],
        );
        let panel = vec![
            Line::from(Span::styled(
                " CAPABILITIES ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  chat  •  tools  •  codebase memory",
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::styled(
                "  commands: /help  /clear  /quit",
                Style::default().fg(theme.muted),
            )),
            Line::from(""),
            Line::from(Span::styled(
                " SESSION ",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "  model: default  •  mode: ready",
                Style::default().fg(theme.foreground),
            )),
            Line::from(Span::styled(
                format!("  {}", TAGLINE),
                Style::default().fg(theme.muted),
            )),
        ];
        frame.render_widget(
            Paragraph::new(panel).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" LUMINUS ")
                    .border_style(Style::default().fg(theme.border)),
            ),
            columns[1],
        );
        return;
    }

    let lines = if compact {
        vec![
            Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(TAGLINE, Style::default().fg(theme.muted))),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                title,
                Style::default()
                    .fg(theme.primary)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(TAGLINE, Style::default().fg(theme.accent))),
            Line::from(Span::styled(
                "  chat  •  tools  •  /help",
                Style::default().fg(theme.muted),
            )),
        ]
    };

    frame.render_widget(
        Paragraph::new(lines).alignment(Alignment::Center).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(theme.border).bg(theme.background)),
        ),
        area,
    );
}

/// Theme color for a tool activity card, keyed by lifecycle status.
///
/// Monochrome themes fall back to foreground/muted so cards stay readable
/// on terminals without color support.
fn tool_status_color(status: ToolStatus, theme: Theme) -> Color {
    if theme.monochrome {
        return match status {
            ToolStatus::Started | ToolStatus::InProgress => theme.muted,
            ToolStatus::Completed | ToolStatus::Failed => theme.foreground,
        };
    }
    match status {
        ToolStatus::Started => theme.muted,
        ToolStatus::InProgress => theme.accent,
        ToolStatus::Completed => theme.primary,
        ToolStatus::Failed => Color::Red,
    }
}

/// Build the lines for the tool activity cards shown after the messages.
///
/// Each card comes from `ToolActivity::card()` (a stable, line-oriented
/// representation). The header line is tinted by status; detail lines use the
/// regular foreground so long output stays readable when wrapped on narrow
/// terminals.
fn tool_activity_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.tool_activities.is_empty() {
        return lines;
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "TOOLS".to_owned(),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )));

    for activity in &app.tool_activities {
        let color = tool_status_color(activity.meta().status, theme);
        let card = activity.card();
        for (index, card_line) in card.lines().enumerate() {
            if index == 0 {
                lines.push(Line::from(Span::styled(
                    card_line.to_owned(),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                )));
            } else {
                lines.push(Line::from(vec![
                    Span::styled("  ".to_owned(), Style::default().fg(color)),
                    Span::styled(card_line.to_owned(), Style::default().fg(theme.foreground)),
                ]));
            }
        }
    }
    lines
}

fn agent_run_lines(app: &App, theme: Theme) -> Vec<Line<'static>> {
    if app.agent_runs.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![Line::from("")];
    lines.push(Line::from(Span::styled(
        "AGENTS".to_owned(),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )));
    for run in &app.agent_runs {
        let (marker, color, detail) = match &run.status {
            AgentStatus::Running => ("●", theme.accent, format!("running: {}", run.prompt)),
            AgentStatus::Completed => ("✓", theme.primary, "completed".to_owned()),
            AgentStatus::Cancelled => ("-", theme.muted, "cancelled".to_owned()),
            AgentStatus::Failed(error) => ("✗", Color::Red, format!("failed: {error}")),
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{marker} agent-{} ", run.agent_id),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ),
            Span::styled(detail, Style::default().fg(theme.foreground)),
        ]));
        if !run.output.is_empty() && !matches!(run.status, AgentStatus::Running) {
            lines.push(Line::from(Span::styled(
                format!("  output: {}", run.output),
                Style::default().fg(theme.foreground),
            )));
        }
    }
    lines
}

fn render_conversation(frame: &mut Frame<'_>, area: Rect, app: &App, theme: Theme) {
    let mut lines: Vec<Line<'static>> = if app.messages.is_empty() {
        vec![Line::from(Span::styled(
            "Ready. Enter a prompt to begin.".to_owned(),
            Style::default().fg(theme.muted),
        ))]
    } else {
        app.messages
            .iter()
            .map(|message| {
                let (label, color) = match message.role {
                    Role::User => ("YOU", theme.accent),
                    Role::Assistant => ("LUMINUS", theme.primary),
                    Role::System => ("SYSTEM", theme.muted),
                };
                Line::from(vec![
                    Span::styled(
                        format!("{label}  "),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        message.content.clone(),
                        Style::default().fg(theme.foreground),
                    ),
                ])
            })
            .collect::<Vec<_>>()
    };

    lines.extend(tool_activity_lines(app, theme));
    lines.extend(agent_run_lines(app, theme));
    let body = Text::from(lines);

    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" CONVERSATION ")
                .border_style(Style::default().fg(theme.border))
                .style(Style::default().bg(theme.background).fg(theme.foreground)),
        ),
        area,
    );
}
