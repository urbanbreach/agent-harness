use super::*;

pub(super) fn render_memory_browser_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if root.width == 0 || root.height == 0 {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let overlay_width = root.width.clamp(40, 80);
    let overlay_height = root.height.clamp(8, 24);
    let overlay_x = root.x + (root.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = root.y + (root.height.saturating_sub(overlay_height)) / 2;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    let surface = ui_chrome::command_palette_surface(theme);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let selected_style = ui_chrome::overlay_focus_row_style(theme);
    let text_style = Style::default().fg(theme.text.primary).bg(surface);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);

    if !paint_command_palette_panel(frame, theme, overlay) {
        return;
    }
    let inner = inset_rect(overlay, 1.min(overlay.width.saturating_sub(1)), 1);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let title = "Memory";
    let title_area = Rect::new(inner.x, inner.y, inner.width, 1);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(title, title_style),
            Span::styled(
                " ".repeat(usize::from(inner.width).saturating_sub(title.len() + 3)),
                Style::default().bg(surface),
            ),
            Span::styled("esc", muted_style),
        ])),
        title_area,
    );

    let width = usize::from(inner.width);
    let list_y = inner.y.saturating_add(1);
    let list_height = inner.height.saturating_sub(1);
    let entries = app.memory_browser.filtered_entries();
    if entries.is_empty() {
        if list_height > 0 {
            let area = Rect::new(inner.x, list_y, inner.width, 1);
            frame.render_widget(
                Paragraph::new(Line::from(Span::styled("No memory entries", muted_style))),
                area,
            );
        }
        return;
    }
    for row_index in 0..usize::from(list_height) {
        let Some(entry) = entries.get(row_index) else {
            break;
        };
        let y = list_y.saturating_add(u16::try_from(row_index).unwrap_or(u16::MAX));
        let area = Rect::new(inner.x, y, inner.width, 1);
        let is_selected = row_index == app.memory_browser.selected;
        let style = if is_selected {
            selected_style
        } else {
            text_style
        };
        let marker = if is_selected { "> " } else { "  " };
        let label = truncate_plain_text(&entry.label, width.saturating_sub(marker.chars().count()));
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(label, style),
            ])),
            area,
        );
    }
}
