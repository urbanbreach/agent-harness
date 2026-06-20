use super::*;

pub(super) fn render_theme_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let dialog_width = 44u16.min(root.width.saturating_sub(4));
    let dialog_height = 8u16.min(root.height.saturating_sub(4));
    let dialog_x = root.x + (root.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = root.y + (root.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    let surface = ui_chrome::command_palette_surface(theme);
    let border_style = Style::default().fg(theme.border.strong).bg(surface);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let row_style = Style::default().bg(surface);
    let selected_style = ui_chrome::overlay_focus_row_style(theme);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(surface)),
        dialog_area,
    );

    let title_area = Rect::new(dialog_x + 1, dialog_y, dialog_width.saturating_sub(2), 1);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(" Themes", title_style))),
        title_area,
    );

    let names = Theme::available_theme_names();
    let list_y = dialog_y + 2;
    for (index, name) in names.iter().enumerate() {
        let row_area = Rect::new(
            dialog_x + 1,
            list_y + u16::try_from(index).unwrap_or(u16::MAX),
            dialog_width.saturating_sub(2),
            1,
        );
        let is_selected = index == app.theme_dialog_selected;
        let is_current = *name == app.theme_name;
        let style = if is_selected {
            selected_style
        } else {
            row_style
        };
        frame.render_widget(Block::default().style(style), row_area);

        let marker = if is_current { "● " } else { "  " };
        let label: &'static str = match *name {
            "default" => "Harness Dark",
            "high-contrast" => "High Contrast",
            _ => name,
        };
        let fg = if is_selected {
            theme.text.inverse
        } else {
            theme.text.primary
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    marker.to_string(),
                    Style::default().fg(fg).bg(style.bg.unwrap_or(surface)),
                ),
                Span::styled(
                    label.to_string(),
                    Style::default().fg(fg).bg(style.bg.unwrap_or(surface)),
                ),
            ])),
            row_area,
        );
    }

    let hint_y = list_y + u16::try_from(names.len()).unwrap_or(u16::MAX) + 1;
    if hint_y < dialog_y + dialog_height {
        let hint_area = Rect::new(dialog_x + 1, hint_y, dialog_width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Enter to apply · Esc to close",
                muted_style,
            ))),
            hint_area,
        );
    }
}
