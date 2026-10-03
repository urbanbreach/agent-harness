use crate::{
    app::{AppState, OrchestrationTaskState},
    text::collapse_inline_whitespace,
    theme::Theme,
};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub(super) fn render(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let layout = app.task_pane_layout(area);
    let rows = app.task_pane_rows();
    let bounds = frame.buffer_mut().area;
    frame.buffer_mut().set_style(
        Rect::new(
            bounds.x,
            area.y.saturating_sub(1),
            bounds.width,
            area.height.saturating_add(2),
        )
        .intersection(bounds),
        Style::default().fg(theme.text.primary),
    );
    if app.tasks_pane.focused || app.tasks_pane.hovered {
        let border = Rect::new(
            area.x.saturating_sub(4),
            area.y.saturating_sub(1),
            area.width.saturating_add(7),
            area.height.saturating_add(2),
        );
        frame.render_widget(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(crate::theme::quantize_color(
                    ratatui::style::Color::Rgb(60, 60, 65),
                    theme.color_level(),
                ))),
            border,
        );
        let style = Style::default().fg(crate::theme::quantize_color(
            ratatui::style::Color::Rgb(60, 60, 65),
            theme.color_level(),
        ));
        for (x, y, glyph) in [
            (border.x, border.y, "┌"),
            (border.x, border.bottom() - 1, "└"),
            (border.right() - 1, border.bottom() - 1, "┘"),
            (border.right() - 1, border.y, "✗"),
        ] {
            frame.buffer_mut()[(x, y)]
                .set_symbol(glyph)
                .set_style(style);
        }
    }
    for (row, glyph) in [(layout.top, "▲"), (layout.bottom, "▼")] {
        if let Some(y) = row {
            frame.render_widget(
                Paragraph::new(glyph).style(Style::default().fg(theme.text.secondary)),
                Rect::new(area.x + area.width / 2, y, 1, 1),
            );
        }
    }
    if let Some(bar) = layout.query {
        super::ui_pane_query::render(frame, &app.tasks_pane.query, bar, theme);
    }
    let area = layout.rows;
    if app.task_pane_all_rows().is_empty() {
        frame.render_widget(
            Paragraph::new(if app.tasks_pane.show_done {
                "No tasks or agents."
            } else {
                "No running tasks. Press h to show all."
            })
            .style(Style::default().fg(theme.text.secondary)),
            area,
        );
        return;
    }
    let offset = layout.offset;
    for (index, row) in rows
        .iter()
        .enumerate()
        .skip(offset)
        .take(usize::from(area.height))
    {
        let y = area.y + u16::try_from(index - offset).unwrap_or(0);
        let selected = app.tasks_pane.focused && index == app.tasks_pane.selected;
        let base = Style::default().bg(if selected {
            theme.surface.selected_card
        } else {
            theme.surface.canvas
        });
        frame.render_widget(
            Paragraph::new(" ".repeat(usize::from(area.width))).style(base),
            Rect::new(area.x, y, area.width, 1),
        );
        if row.header {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled(
                        row.title.chars().take(2).collect::<String>(),
                        base.fg(theme.text.secondary),
                    ),
                    Span::styled(
                        row.group,
                        base.fg(theme.text.tertiary).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        row.title
                            .split_once(row.group)
                            .map_or("", |(_, count)| count)
                            .to_owned(),
                        base.fg(theme.text.secondary),
                    ),
                ]))
                .style(base),
                Rect::new(area.x, y, area.width, 1),
            );
            continue;
        }
        let running = !row.state.is_terminal();
        let (_, color) = task_icon(app, row, theme);
        let title_width = area.width.saturating_sub(2);
        let spans = title_spans(row, running, color, base, theme, usize::from(title_width));
        let spans = highlight_title(spans, &app.tasks_pane.query);
        frame.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(area.x + 2, y, title_width, 1),
        );
    }
    // Native overlays retain the unfiltered row positions during filtering.
    for (index, row) in app
        .task_pane_all_rows()
        .iter()
        .enumerate()
        .skip(offset)
        .take(usize::from(area.height))
    {
        if row.header {
            continue;
        }
        let y = area.y + u16::try_from(index - offset).unwrap_or(0);
        let base = Style::default();
        let running = !row.state.is_terminal();
        let (icon, color) = task_icon(app, row, theme);
        let right = format!(
            "{}{}{} [↗]{}",
            collapse_inline_whitespace(&row.model),
            if row.model.is_empty() { "" } else { " " },
            crate::app::subagents::duration_label(row.elapsed_ms),
            if running { "[✗]" } else { "" }
        );
        let badge = if row.badge.is_empty() {
            String::new()
        } else {
            format!("{} ", row.badge)
        };
        let right_width = u16::try_from(right.width() + badge.width())
            .unwrap_or(u16::MAX)
            .min(area.width.saturating_sub(2));
        clear_overlay(frame, area, y, right_width.saturating_add(1));
        frame.render_widget(
            Paragraph::new(icon).style(base.fg(color)),
            Rect::new(area.x, y, 1, 1),
        );
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    badge,
                    base.fg(theme.text.secondary).add_modifier(Modifier::DIM),
                ),
                Span::styled(right, base.fg(theme.text.secondary)),
            ])),
            Rect::new(area.right().saturating_sub(right_width), y, right_width, 1),
        );
    }
}

fn task_icon<'a>(
    app: &AppState,
    row: &crate::app::tasks_pane::TaskPaneRow,
    theme: &'a Theme,
) -> (&'a str, ratatui::style::Color) {
    if !row.state.is_terminal() {
        let frames = ["⋅", ":", "⸬", "⁙"];
        (
            frames[(app.transcript_animation_phase() / 4) % frames.len()],
            theme.text.accent,
        )
    } else if row.state == OrchestrationTaskState::Completed {
        ("✓", theme.status.success)
    } else {
        ("✗", theme.status.error)
    }
}

fn title_spans(
    row: &crate::app::tasks_pane::TaskPaneRow,
    running: bool,
    color: ratatui::style::Color,
    base: Style,
    theme: &Theme,
    width: usize,
) -> Vec<Span<'static>> {
    let label = if row.label.is_empty() {
        String::new()
    } else {
        format!("{} ", collapse_inline_whitespace(&row.label))
    };
    let description = collapse_inline_whitespace(
        row.title
            .strip_prefix(&row.label)
            .unwrap_or(&row.title)
            .trim(),
    );
    let activity = row.activity.as_deref().filter(|_| running);
    let description = super::truncate_plain_text(
        &description,
        if activity.is_some() {
            40.min(width.saturating_sub(label.width()))
        } else {
            width.saturating_sub(label.width())
        },
    );
    let mut spans = vec![
        Span::styled(
            label,
            base.fg(if running {
                color
            } else {
                super::ui_transcript_style::blend_color(theme.surface.canvas, color, 0.45)
            }),
        ),
        Span::styled(
            description,
            base.fg(if running {
                theme.text.primary
            } else {
                theme.text.tertiary
            }),
        ),
    ];
    if let Some(activity) = activity {
        let used: usize = spans.iter().map(|span| span.width()).sum();
        let text = super::truncate_plain_text(
            &format!(" · {}", collapse_inline_whitespace(activity)),
            width.saturating_sub(used),
        );
        spans.push(Span::styled(text, base.fg(theme.text.secondary)));
    }
    spans
}

fn highlight_title(
    spans: Vec<Span<'static>>,
    query: &crate::app::pane_query::PaneQuery,
) -> Vec<Span<'static>> {
    let Some(regex) = query
        .regex
        .as_ref()
        .filter(|_| query.editing || query.mode == crate::app::pane_query::PaneQueryMode::Search)
    else {
        return spans;
    };
    let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
    let matches: Vec<_> = regex
        .find_iter(&text)
        .map(|matched| matched.range())
        .collect();
    let mut result = Vec::new();
    let mut start = 0;
    for span in spans {
        let end = start + span.content.len();
        let mut cursor = start;
        for matched in matches
            .iter()
            .filter(|range| range.start < end && range.end > start)
        {
            let lo = matched.start.max(start);
            let hi = matched.end.min(end);
            result.push(Span::styled(
                span.content[cursor - start..lo - start].to_owned(),
                span.style,
            ));
            result.push(Span::styled(
                span.content[lo - start..hi - start].to_owned(),
                span.style.add_modifier(Modifier::REVERSED),
            ));
            cursor = hi;
        }
        result.push(Span::styled(
            span.content[cursor - start..].to_owned(),
            span.style,
        ));
        start = end;
    }
    result
}

pub(super) fn render_footer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let key = Style::default()
        .fg(theme.markdown.text)
        .add_modifier(Modifier::BOLD);
    let label = Style::default().fg(theme.text.secondary);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("h", key),
            Span::styled(
                if app.tasks_pane.show_done {
                    ":hide done"
                } else {
                    ":show done"
                },
                label,
            ),
            Span::styled("  │  ", label.add_modifier(Modifier::DIM)),
            Span::styled(
                if app.shortcuts_ctrl_dot {
                    "Ctrl+."
                } else {
                    "Ctrl+x"
                },
                key,
            ),
            Span::styled(":shortcuts", label),
        ])),
        area,
    );
}

fn clear_overlay(frame: &mut Frame, area: Rect, y: u16, width: u16) {
    let width = width.min(area.width);
    let x = area.right().saturating_sub(width);
    let buffer = frame.buffer_mut();
    let ellipsis = x > area.x
        && buffer
            .cell((x, y))
            .is_some_and(|cell| !cell.symbol().trim().is_empty());
    if ellipsis {
        if let Some(cell) = buffer.cell_mut((x - 1, y)) {
            cell.set_symbol("…");
        }
    }
    buffer.set_span(x, y, &Span::raw(" ".repeat(usize::from(width))), width);
}
