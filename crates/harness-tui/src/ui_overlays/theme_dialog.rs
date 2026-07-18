use super::*;

pub(super) fn render_theme_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let dialog_width = 44u16.min(root.width.saturating_sub(4));
    let dialog_height = 8u16.min(root.height.saturating_sub(4));
    if dialog_width < 32 || dialog_height < 6 {
        return;
    }
    let dialog_x = root.x + (root.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = root.y + (root.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    render_overlay_dim_backdrop(frame, root);
    if !paint_command_palette_panel(frame, theme, dialog_area) {
        return;
    }

    let content = inset_rect(dialog_area, 2.min(dialog_area.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content);
    render_command_palette_header(frame, theme, chunks[0], "Themes");
    render_theme_dialog_body(frame, app, theme, chunks[1]);
}

fn render_theme_dialog_body(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let names = Theme::available_theme_names();
    let name_count = u16::try_from(names.len()).unwrap_or(u16::MAX);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(name_count),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), chunks[0]);

    for (index, name) in names.iter().enumerate() {
        let row_area = Rect::new(
            chunks[1].x,
            chunks[1]
                .y
                .saturating_add(u16::try_from(index).unwrap_or(u16::MAX)),
            chunks[1].width,
            1,
        );
        let is_selected = index == app.theme_dialog_selected;
        let is_current = *name == app.theme_name;
        let row_style = if is_selected {
            ui_chrome::overlay_focus_row_style(theme)
        } else {
            Style::default().bg(surface)
        };
        frame.render_widget(Block::default().style(row_style), row_area);

        let prefix = "  ";
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
        let bg = row_style.bg.unwrap_or(surface);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(prefix, Style::default().fg(fg).bg(bg)),
                Span::styled(marker, Style::default().fg(fg).bg(bg)),
                Span::styled(label, Style::default().fg(fg).bg(bg)),
            ])),
            row_area,
        );
    }

    frame.render_widget(Block::default().style(Style::default().bg(surface)), chunks[2]);

    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "enter apply · esc close",
            muted_style,
        ))),
        chunks[3],
    );
}
