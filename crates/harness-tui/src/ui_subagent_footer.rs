use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::app::{AppState, SubagentSessionInfo};
use crate::theme::Theme;

use super::muted_meta_style;
use super::ui_subagent_footer_navigation::{
    subagent_navigation_items, subagent_navigation_line, subagent_navigation_width,
};

pub(super) fn render_subagent_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    _text_area: Rect,
    theme: &Theme,
    info: &SubagentSessionInfo,
) {
    let surface = theme.surface.panel;
    let style = Style::default().bg(surface);
    frame.render_widget(Block::default().style(style), area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    let content_y = if area.height > 1 {
        area.y.saturating_add(1)
    } else {
        area.y
    };
    let content_x = area.x.saturating_add(2);
    let content_right_padding = 1;
    let used_left = content_x.saturating_sub(area.x);
    let content_width = area
        .width
        .saturating_sub(used_left)
        .saturating_sub(content_right_padding);
    if content_width == 0 {
        return;
    }

    let nav_items = subagent_navigation_items(app);
    let nav_width = subagent_navigation_width(&nav_items).min(usize::from(content_width));
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(u16::try_from(nav_width).unwrap_or(u16::MAX)),
        ])
        .split(Rect::new(content_x, content_y, content_width, 1));

    if columns[0].width > 0 {
        frame.render_widget(
            Paragraph::new(subagent_info_line(info, theme, surface)).style(style),
            columns[0],
        );
    }
    if columns[1].width > 0 {
        frame.render_widget(
            Paragraph::new(subagent_navigation_line(
                &nav_items,
                app.hovered_subagent_footer_target,
                theme,
                surface,
            ))
            .style(Style::default().fg(theme.text.primary).bg(surface)),
            columns[1],
        );
    }
}

fn subagent_info_line(
    info: &SubagentSessionInfo,
    theme: &Theme,
    surface: ratatui::style::Color,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        info.label.clone(),
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(count) = subagent_count_text(info) {
        spans.push(Span::styled(
            format!(" {count}"),
            muted_meta_style(theme).bg(surface),
        ));
    }
    if let Some(usage) = info.usage.as_deref() {
        spans.push(Span::styled(
            format!("  {usage}"),
            muted_meta_style(theme).bg(surface),
        ));
    }
    Line::from(spans)
}

fn subagent_count_text(info: &SubagentSessionInfo) -> Option<String> {
    (info.total > 0).then(|| format!("({} of {})", info.index, info.total))
}
