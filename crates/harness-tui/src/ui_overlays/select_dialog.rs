use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SelectDialogLayout {
    pub(super) header: Rect,
    pub(super) input: Rect,
    pub(super) list: Rect,
}

pub(super) fn render_select_dialog_surface(
    frame: &mut Frame,
    theme: &Theme,
    overlay: Rect,
) -> Option<Rect> {
    if !paint_select_dialog_panel(frame, theme, overlay) {
        return None;
    }

    let content = inset_rect(overlay, 3.min(overlay.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return None;
    }

    Some(content)
}

pub(super) fn paint_select_dialog_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        overlay,
    );
    true
}

pub(super) fn select_dialog_layout(overlay: Rect) -> Option<SelectDialogLayout> {
    if overlay.width <= 8 || overlay.height <= 6 {
        return None;
    }

    let content_x = overlay.x.saturating_add(4);
    let content_width = overlay.width.saturating_sub(8);
    let header = Rect::new(content_x, overlay.y.saturating_add(1), content_width, 1);
    let input = Rect::new(content_x, overlay.y.saturating_add(3), content_width, 1);
    let list = Rect::new(
        overlay.x,
        overlay.y.saturating_add(5),
        overlay.width,
        overlay.height.saturating_sub(6),
    );
    Some(SelectDialogLayout {
        header,
        input,
        list,
    })
}

pub(super) fn render_select_dialog_header(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let esc = "esc";
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(esc.chars().count() as u16),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new(title.to_string()).style(
            Style::default()
                .fg(ui_chrome::command_palette_title(theme))
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(esc).alignment(Alignment::Right).style(
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(surface),
        ),
        columns[1],
    );
}

pub(super) fn render_select_dialog_empty_message(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    message: &str,
) {
    let empty_area = Rect::new(
        area.x.saturating_add(3),
        area.y,
        area.width.saturating_sub(3),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_plain_text(message, usize::from(empty_area.width)),
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(ui_chrome::command_palette_surface(theme)),
        ))),
        empty_area,
    );
}

pub(super) fn render_select_dialog_input(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    input: &str,
    cursor: usize,
    placeholder: &str,
    cursor_color: Color,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let line = if input.is_empty() {
        Line::from(vec![
            Span::styled("█", Style::default().fg(cursor_color).bg(surface)),
            Span::styled(
                format!(" {placeholder}"),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
        ])
    } else {
        let cursor_byte = input
            .char_indices()
            .nth(cursor)
            .map(|(index, _)| index)
            .unwrap_or(input.len());
        let before = &input[..cursor_byte];
        let after = &input[cursor_byte..];
        Line::from(vec![
            Span::styled(
                before.to_string(),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
            Span::styled("█", Style::default().fg(cursor_color).bg(surface)),
            Span::styled(
                after.to_string(),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}
