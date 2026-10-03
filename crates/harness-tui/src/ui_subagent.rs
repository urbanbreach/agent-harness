use ratatui::{
    layout::{Position, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{ActivityStatus, AppState},
    layout::{subagent, FrameLayoutPlan},
    theme::Theme,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubagentFrameTarget {
    Close,
}

pub(crate) fn subagent_frame_target_at(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<SubagentFrameTarget> {
    if !app.current_subagent_session_present() || app.review_surface().is_some() {
        return None;
    }
    subagent::close(area)?
        .contains(Position::new(column, row))
        .then_some(SubagentFrameTarget::Close)
}

pub(super) fn render(frame: &mut Frame, app: &AppState, plan: &FrameLayoutPlan, theme: &Theme) {
    frame
        .buffer_mut()
        .set_style(plan.root, Style::default().fg(theme.text.primary));
    let Some(area) = subagent::frame(plan.root) else {
        return;
    };
    let style = Style::default()
        .fg(crate::theme::quantize_color(
            ratatui::style::Color::Rgb(60, 60, 65),
            theme.color_level(),
        ))
        .bg(theme.surface.canvas);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(style),
        Rect::new(area.x, area.y, area.width, 2),
    );
    let content = Rect::new(area.x, area.y + 2, area.width, area.height - 2);
    frame.render_widget(
        Block::default().borders(Borders::ALL).border_style(style),
        content,
    );
    for (x, glyph) in [(area.x, "├"), (area.right() - 1, "┤")] {
        frame.buffer_mut()[(x, content.y)]
            .set_symbol(glyph)
            .set_style(style);
    }
    let Some(header) = app.subagent_header() else {
        return;
    };
    frame.buffer_mut().set_style(
        plan.shell,
        Style::default().fg(ratatui::style::Color::Reset),
    );
    render_title(frame, app, area, &header, theme);
    let cwd = crate::text::collapse_inline_whitespace(&header.cwd);
    let cwd_width = u16::try_from(cwd.width())
        .unwrap_or(u16::MAX)
        .min(plan.header.width);
    frame.render_widget(
        Paragraph::new(cwd).style(Style::default().fg(theme.markdown.text)),
        Rect {
            width: cwd_width,
            ..plan.header
        },
    );
    render_link_preview(frame, app, plan.header, theme);
    if let Some(transcript) = plan.transcript {
        super::render_transcript_pane(frame, app, transcript, theme);
    }
    if let Some(index) = app.transcript_view.highlighted_link {
        for (_, link) in app
            .transcript_view
            .hyperlinks
            .iter()
            .skip(index)
            .enumerate()
            .take_while(|(offset, link)| *offset == 0 || link.continues_previous)
        {
            frame.buffer_mut().set_style(
                Rect::new(
                    link.start_column,
                    link.row,
                    link.end_column.saturating_sub(link.start_column),
                    1,
                ),
                Style::default()
                    .fg(theme.markdown.link)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD),
            );
        }
    }
    if let Some(status) = plan.status.filter(|_| {
        matches!(
            header.status,
            ActivityStatus::Queued | ActivityStatus::Streaming
        )
    }) {
        render_status(frame, app, status, &header, theme);
    }
    let mut shortcuts = vec![("q/Esc", "back"), ("Enter", "expand")];
    if app.composer.vim_mode {
        shortcuts.extend([("j/k", "nav"), ("Shift+l/h", "turn")]);
    }
    shortcuts.extend([
        (
            "Ctrl+e",
            if app.transcript_view.show_transcript_thinking {
                "collapse thinking"
            } else {
                "expand thinking"
            },
        ),
        ("Ctrl+c", "cancel"),
    ]);
    shortcuts.truncate(5);
    shortcuts.push((
        if app.shortcuts_ctrl_dot {
            "Ctrl+."
        } else {
            "Ctrl+x"
        },
        "shortcuts",
    ));
    let mut spans = Vec::new();
    for (index, (key, label)) in shortcuts.into_iter().enumerate() {
        if index > 0 {
            spans.push(Span::styled(
                "  │  ",
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::DIM),
            ));
        }
        for (part_index, part) in key.split('/').enumerate() {
            if part_index > 0 {
                spans.push(Span::styled("/", Style::default().fg(theme.text.secondary)));
            }
            spans.push(Span::styled(
                part,
                Style::default()
                    .fg(theme.markdown.text)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        let used: usize = spans.iter().map(Span::width).sum();
        let label = if used + label.width() < usize::from(plan.footer.width) {
            label
        } else {
            ""
        };
        spans.push(Span::styled(
            format!(":{label}"),
            Style::default().fg(theme.text.secondary),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), plan.footer);
}

fn render_link_preview(frame: &mut Frame, app: &AppState, header: Rect, theme: &Theme) {
    let Some(link) = app
        .transcript_view
        .highlighted_link
        .and_then(|index| app.transcript_view.hyperlinks.get(index))
    else {
        return;
    };
    let available = header.width.saturating_sub(24);
    if available < 12 {
        return;
    }
    let text = super::ui_chrome::truncate_plain_text(&link.destination, usize::from(available));
    let width = u16::try_from(text.width()).unwrap_or(available);
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(theme.markdown.link)
                .bg(theme.surface.canvas),
        ),
        Rect::new(header.right().saturating_sub(width), header.y, width, 1),
    );
}

fn render_status(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    header: &crate::app::subagents::SubagentHeader,
    theme: &Theme,
) {
    let tool_title = header.activity.strip_prefix("Running: ");
    let frames = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let icon = frames[(app.transcript_animation_phase() / 4) % frames.len()];
    let text = Line::from(vec![
        Span::styled(
            format!("{icon} "),
            Style::default().fg(theme.status.success),
        ),
        Span::styled(
            tool_title.map_or_else(|| header.activity.clone(), |_| "Run".into()) + " ",
            Style::default().fg(theme.text.secondary),
        ),
        Span::styled(
            tool_title.unwrap_or_default().to_owned(),
            Style::default().fg(theme.agents.build),
        ),
        Span::styled(
            format!(
                " {}",
                crate::app::subagents::duration_label(header.phase_elapsed_ms)
            ),
            Style::default().fg(theme.text.secondary),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(text),
        Rect::new(
            area.x + 2,
            area.y,
            area.width.saturating_sub(12),
            area.height,
        ),
    );
    frame.render_widget(
        Paragraph::new(" [stop]").style(Style::default().fg(theme.text.secondary)),
        Rect::new(
            area.right().saturating_sub(7),
            area.y,
            7.min(area.width),
            area.height,
        ),
    );
}

fn render_title(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    header: &crate::app::subagents::SubagentHeader,
    theme: &Theme,
) {
    let running = matches!(
        header.status,
        ActivityStatus::Queued | ActivityStatus::Streaming
    );
    let (icon, color) = if running {
        let frames = ["⋅", ":", "⸬", "⁙", "⋅", ":", "⸬", "⁙"];
        (
            frames[(app.transcript_view.transcript_animation_phase / 4) % frames.len()],
            theme.text.accent,
        )
    } else if header.status == ActivityStatus::Done {
        ("✓", theme.status.success)
    } else {
        ("✗", theme.status.error)
    };
    let elapsed = crate::app::subagents::duration_label(header.elapsed_ms);
    let activity = if running {
        format!(
            "{} · ",
            crate::text::collapse_inline_whitespace(&header.activity)
        )
    } else {
        String::new()
    };
    let badge = if header.badge.is_empty() {
        String::new()
    } else {
        format!("{} ", header.badge)
    };
    let right = format!("{badge}{activity}{elapsed} ");
    let close_x = area.right().saturating_sub(5);
    let right_width = u16::try_from(right.width()).unwrap_or(u16::MAX);
    let right_x = close_x.saturating_sub(right_width).max(area.x + 1);
    let label = format!(
        "{} ",
        crate::text::collapse_inline_whitespace(&header.label)
    );
    let model = if header.model.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            crate::text::collapse_inline_whitespace(&header.model)
        )
    };
    let budget = usize::from(right_x.saturating_sub(area.x + 4))
        .saturating_sub(label.width() + model.width());
    let description = super::truncate_plain_text(
        &crate::text::collapse_inline_whitespace(&header.description),
        budget,
    );
    let title = Line::from(vec![
        Span::styled(format!(" {icon}"), Style::default().fg(color)),
        Span::styled(" ", Style::default().fg(ratatui::style::Color::Reset)),
        Span::styled(label, Style::default().fg(color)),
        Span::styled(
            description,
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(model, Style::default().fg(theme.text.secondary)),
    ]);
    frame.render_widget(
        Paragraph::new(title),
        Rect::new(
            area.x + 1,
            area.y + 1,
            right_x.saturating_sub(area.x + 1),
            1,
        ),
    );
    frame.render_widget(
        Paragraph::new(right).style(Style::default().fg(theme.text.secondary)),
        Rect::new(right_x, area.y + 1, close_x.saturating_sub(right_x), 1),
    );
    let close_style = if app.hovered_subagent_frame_target.is_some() {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.secondary)
    };
    frame.render_widget(
        Paragraph::new("[✗]").style(close_style),
        Rect::new(close_x, area.y + 1, 3, 1),
    );
}
