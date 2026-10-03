use super::{layout::ViewerLayout, render::viewer_secondary, ViewerRenderSurface};
use crate::theme::Theme;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Clear, Paragraph, Widget},
};

pub(super) fn paint_status(
    buffer: &mut Buffer,
    layout: &ViewerLayout,
    body: Rect,
    surface: &ViewerRenderSurface,
    theme: &Theme,
) {
    if surface.search_active || surface.visual_mode {
        let y = layout.body.bottom().saturating_sub(2);
        let divider = Rect::new(
            layout.popup.x + 1,
            y,
            layout.popup.width.saturating_sub(2),
            1,
        );
        Clear.render(divider, buffer);
        buffer.set_style(divider, Style::default().bg(theme.surface.shell));
        buffer.set_string(
            layout.popup.x + 1,
            y,
            "─".repeat(usize::from(layout.popup.width.saturating_sub(2))),
            Style::default().fg(theme.terminal_colors.muted),
        );
        let status_area = Rect::new(
            body.x,
            y + 1,
            body.width + if surface.child { 0 } else { 2 },
            1,
        );
        Clear.render(status_area, buffer);
        let secondary = viewer_secondary(theme);
        let status = if surface.editing {
            let prefix = if surface.filtering {
                "filter: "
            } else {
                "search: "
            };
            let value = surface.status.strip_prefix(prefix).unwrap_or_default();
            crate::ui::query_editing_line(
                value,
                surface.input_cursor,
                if surface.filtering {
                    "filter"
                } else {
                    "search"
                },
                status_area.width,
                theme,
            )
        } else {
            let style = Style::default().fg(secondary);
            ratatui::text::Line::styled(
                surface.status.clone(),
                if surface.search_active {
                    style.add_modifier(Modifier::DIM)
                } else {
                    style
                },
            )
        };
        Paragraph::new(status)
            .alignment(if surface.search_active && !surface.editing {
                ratatui::layout::Alignment::Right
            } else {
                ratatui::layout::Alignment::Left
            })
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(theme.surface.shell),
            )
            .render(status_area, buffer);
    }
}
