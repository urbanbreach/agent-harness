use super::*;

use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::app::{HelpMode, ModalSurfaceKey, ModalTarget};

#[path = "ui_secondary_events_tab/rows.rs"]
mod rows;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HelpModalRects {
    pub(crate) popup: Rect,
    pub(crate) search: Rect,
    pub(crate) list: Rect,
    pub(crate) primary_footer: Rect,
    pub(crate) secondary_footer: Rect,
}

pub(super) fn render_help_tab(
    frame: &mut Frame,
    app: &AppState,
    root: Rect,
    _content: Rect,
    composer: Option<Rect>,
    theme: &Theme,
) {
    let Some(layout) = help_modal_rects(root, composer) else {
        return;
    };
    paint_panel(frame, app, theme, layout.popup);
    match app.help_detail() {
        Some((action, scroll)) => {
            render_detail(frame, app, theme, layout, action, scroll);
            render_detail_footer(frame, theme, layout);
        }
        None => {
            render_search(frame, app, theme, layout.search);
            rows::render_browse(frame, app, theme, layout.list);
            render_browse_footer(frame, app, theme, layout);
        }
    }
}

pub(crate) fn help_modal_rects(root: Rect, composer: Option<Rect>) -> Option<HelpModalRects> {
    let tokens = crate::layout::HELP_MODAL_LAYOUT;
    let max_width = root.width.saturating_sub(4).min(tokens.max_width);
    let preferred_width = u16::try_from(
        u32::from(root.width).saturating_mul(tokens.width_numerator) / tokens.width_denominator,
    )
    .unwrap_or(u16::MAX);
    let width = preferred_width
        .min(max_width)
        .max(tokens.min_width)
        .min(root.width);
    let default_height = root
        .height
        .saturating_sub(tokens.vertical_margin.saturating_mul(2));
    if width < 20 || default_height < 6 {
        return None;
    }
    let x = root.x.saturating_add(root.width.saturating_sub(width) / 2);
    let y = root
        .y
        .saturating_add(root.height.saturating_sub(default_height) / 2);
    let root_bottom = root.y.saturating_add(root.height.saturating_sub(1));
    let default_bottom = y.saturating_add(default_height.saturating_sub(1));
    let bottom = composer
        .map(|area| area.y.saturating_add(1).min(root_bottom))
        .filter(|candidate| *candidate >= y)
        .map_or(default_bottom, |candidate| candidate.min(default_bottom));
    let popup = Rect::new(x, y, width, bottom.saturating_sub(y).saturating_add(1));
    modal_inner_rects(popup)
}

fn modal_inner_rects(popup: Rect) -> Option<HelpModalRects> {
    if popup.width <= 8 || popup.height <= 6 {
        return None;
    }
    let inner = Rect::new(
        popup.x.saturating_add(1),
        popup.y.saturating_add(1),
        popup.width.saturating_sub(2),
        popup.height.saturating_sub(2),
    );
    let content_x = inner.x.saturating_add(2);
    let content_width = inner.width.saturating_sub(4);
    let secondary_footer = Rect::new(
        content_x,
        inner.y.saturating_add(inner.height.saturating_sub(1)),
        content_width,
        1,
    );
    let primary_footer = Rect::new(
        content_x,
        secondary_footer.y.saturating_sub(1),
        content_width,
        1,
    );
    let list_y = inner.y.saturating_add(3);
    let list = Rect::new(
        inner.x,
        list_y,
        inner.width,
        primary_footer.y.saturating_sub(1).saturating_sub(list_y),
    );
    (list.height > 0).then_some(HelpModalRects {
        popup,
        search: Rect::new(content_x, inner.y.saturating_add(1), content_width, 1),
        list,
        primary_footer,
        secondary_footer,
    })
}

fn paint_panel(frame: &mut Frame, app: &AppState, theme: &Theme, popup: Rect) {
    let colors = theme.reference_terminal;
    let panel = Style::default().fg(colors.primary).bg(colors.canvas);
    let border = panel.fg(colors.muted);
    let close_hovered = app.modal_target_hovered(ModalSurfaceKey::Help, ModalTarget::Close);
    let close = if close_hovered {
        panel.add_modifier(Modifier::BOLD)
    } else {
        border
    };
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Plain)
            .border_style(border)
            .style(panel)
            .title(Line::from(vec![
                Span::styled("─ ", border),
                Span::styled("Keyboard Shortcuts", panel.add_modifier(Modifier::BOLD)),
                Span::styled(" ", border),
            ]))
            .title(
                Line::from(vec![
                    Span::styled(" [", close),
                    Span::styled("✗", close),
                    Span::styled("] ─", close),
                ])
                .right_aligned(),
            ),
        popup,
    );
}

fn render_search(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let colors = theme.reference_terminal;
    let muted = Style::default().fg(colors.muted).bg(colors.canvas);
    let primary = Style::default().fg(colors.primary).bg(colors.canvas);
    let searching = app.help_browser.search_active || !app.help_browser.query.is_empty();
    let line = if searching {
        Line::from(vec![
            Span::styled(" search: ", muted),
            Span::styled(app.help_browser.query.clone(), primary),
            Span::styled(" ", Style::default().fg(colors.canvas).bg(colors.primary)),
        ])
    } else {
        Line::from(Span::styled(" / to search", muted))
    };
    frame.render_widget(Paragraph::new(line), area);
    let divider = Rect::new(
        area.x.saturating_sub(2),
        area.y.saturating_add(1),
        area.width.saturating_add(4),
        1,
    );
    frame.render_widget(
        Paragraph::new("─".repeat(usize::from(divider.width))).style(muted),
        divider,
    );
}

fn render_detail(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    layout: HelpModalRects,
    action: Action,
    scroll: usize,
) {
    let colors = theme.reference_terminal;
    let label = action.metadata_label();
    let description = action.metadata_description();
    let mut lines = vec![
        Line::from(Span::styled(
            label,
            Style::default()
                .fg(colors.primary)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            app.keymap.get_binding_strs(action).join(" / "),
            Style::default().fg(colors.secondary),
        )),
    ];
    if description != label {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            description,
            Style::default().fg(colors.primary),
        )));
    }
    if app.help_rows().iter().any(|row| matches!(row, crate::app::HelpRow::Shortcut { action: candidate, dimmed: true, .. } if *candidate == action)) {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "(not active in current context)",
            Style::default().fg(colors.muted),
        )));
    }
    let area = Rect::new(
        layout.search.x,
        layout.search.y,
        layout.search.width,
        layout.list.bottom().saturating_sub(layout.search.y),
    );
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).scroll((
            u16::try_from(scroll.min(help_detail_max_scroll(app, layout))).unwrap_or(u16::MAX),
            0,
        )),
        area,
    );
}

pub(crate) fn help_detail_max_scroll(app: &AppState, layout: HelpModalRects) -> usize {
    let Some((action, _)) = app.help_detail() else {
        return 0;
    };
    let width = layout.search.width;
    let mut content_rows = rows::wrapped_lines(action.metadata_label(), width).len();
    content_rows = content_rows.saturating_add(
        rows::wrapped_lines(&app.keymap.get_binding_strs(action).join(" / "), width).len(),
    );
    if action.metadata_description() != action.metadata_label() {
        content_rows = content_rows
            .saturating_add(1)
            .saturating_add(rows::wrapped_lines(action.metadata_description(), width).len());
    }
    if app.help_rows().iter().any(|row| matches!(row, crate::app::HelpRow::Shortcut { action: candidate, dimmed: true, .. } if *candidate == action)) {
        content_rows = content_rows
            .saturating_add(1)
            .saturating_add(rows::wrapped_lines("(not active in current context)", width).len());
    }
    let visible_rows = usize::from(layout.list.bottom().saturating_sub(layout.search.y));
    content_rows.saturating_sub(visible_rows)
}

fn render_browse_footer(frame: &mut Frame, app: &AppState, theme: &Theme, layout: HelpModalRects) {
    let filter = if app.help_browser.hide_dimmed {
        "f show all"
    } else {
        "f filter"
    };
    let primary = format!("↑/↓ nav  |  {filter}  |  e/Space/→ expand  |  ← collapse");
    let secondary = "Enter details  |  / search  |  Esc close";
    if UnicodeWidthStr::width(primary.as_str()) <= usize::from(layout.primary_footer.width) {
        render_footer_line(frame, theme, layout.primary_footer, &primary);
        render_footer_line(frame, theme, layout.secondary_footer, secondary);
    } else {
        render_footer_line(
            frame,
            theme,
            layout.primary_footer,
            &format!("↑/↓ nav | {filter} | e/Space/→ expand"),
        );
        render_footer_line(
            frame,
            theme,
            layout.secondary_footer,
            "← collapse | Enter details | / search | Esc close",
        );
    }
}

fn render_detail_footer(frame: &mut Frame, theme: &Theme, layout: HelpModalRects) {
    let footer = "Esc back  |  ↑/↓ scroll  |  Ctrl+./X close";
    if UnicodeWidthStr::width(footer) <= usize::from(layout.primary_footer.width) {
        render_footer_line(frame, theme, layout.primary_footer, footer);
    } else {
        render_footer_line(frame, theme, layout.primary_footer, "Esc back | ↑/↓ scroll");
        render_footer_line(frame, theme, layout.secondary_footer, "Ctrl+./X close");
    }
}

fn render_footer_line(frame: &mut Frame, theme: &Theme, area: Rect, text: &str) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            text,
            Style::default().fg(theme.reference_terminal.secondary),
        ))
        .alignment(Alignment::Center),
        area,
    );
}

pub(crate) use rows::help_row_layout;

#[cfg(test)]
#[path = "ui_secondary_events_tab/tests.rs"]
mod tests;
