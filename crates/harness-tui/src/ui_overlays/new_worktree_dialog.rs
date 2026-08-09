use super::*;
use ratatui::widgets::BorderType;

const MIN_DIALOG_WIDTH: u16 = 50;
const DIALOG_HEIGHT: u16 = 5;
const INNER_PAD: u16 = 2;
const LABEL: &str = "Name (optional): ";

pub(super) fn render_new_worktree_dialog(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    if root.width < 20 || root.height < DIALOG_HEIGHT {
        frame.render_widget(
            Paragraph::new("[Esc] to close").style(Style::default().fg(theme.text.tertiary)),
            Rect::new(root.x, root.y, root.width.min(16), 1),
        );
        return;
    }

    let typed_width = unicode_width::UnicodeWidthStr::width(app.new_worktree_dialog.input.as_str());
    let desired = usize::from(MIN_DIALOG_WIDTH)
        .max(LABEL.len().saturating_add(typed_width).saturating_add(6));
    let max_width = root.width.saturating_sub(4);
    let width = u16::try_from(desired)
        .unwrap_or(u16::MAX)
        .min(max_width)
        .max(1);
    let dialog = crate::layout::centered_overlay_area(root, width, DIALOG_HEIGHT);
    let surface = theme.surface.overlay;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border.subtle).bg(surface))
        .style(Style::default().bg(surface));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);

    let content = Rect::new(
        inner.x.saturating_add(INNER_PAD.saturating_sub(1)),
        inner.y,
        inner.width.saturating_sub(INNER_PAD),
        inner.height,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Create worktree",
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ))),
        Rect::new(content.x, content.y, content.width, 1),
    );

    let label_width = unicode_width::UnicodeWidthStr::width(LABEL);
    let input_width = usize::from(content.width).saturating_sub(label_width);
    let (visible, cursor_column) = input_viewport(
        &app.new_worktree_dialog.input,
        app.new_worktree_dialog.cursor,
        input_width,
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(LABEL, Style::default().fg(theme.text.secondary).bg(surface)),
            Span::styled(visible, Style::default().fg(theme.text.primary).bg(surface)),
        ])),
        Rect::new(content.x, content.y.saturating_add(1), content.width, 1),
    );

    let cursor_x = content
        .x
        .saturating_add(u16::try_from(label_width + cursor_column).unwrap_or(u16::MAX))
        .min(content.right().saturating_sub(1));
    if let Some(cell) = frame
        .buffer_mut()
        .cell_mut((cursor_x, content.y.saturating_add(1)))
    {
        cell.set_style(Style::default().fg(surface).bg(theme.text.primary));
    }
    frame.set_cursor_position((cursor_x, content.y.saturating_add(1)));

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                "enter",
                Style::default()
                    .fg(theme.text.accent)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " = create   ",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
            Span::styled(
                "esc",
                Style::default()
                    .fg(theme.text.accent)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " = cancel",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
        ])),
        Rect::new(content.x, content.y.saturating_add(2), content.width, 1),
    );
}

fn input_viewport(input: &str, cursor: usize, width: usize) -> (String, usize) {
    if width == 0 {
        return (String::new(), 0);
    }
    let cursor = cursor.min(input.len());
    let prefix = &input[..cursor];
    let mut start = cursor;
    let mut cursor_column = 0usize;
    for (byte_index, character) in prefix.char_indices().rev() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if cursor_column.saturating_add(character_width) > width.saturating_sub(1) {
            break;
        }
        cursor_column = cursor_column.saturating_add(character_width);
        start = byte_index;
    }
    let mut visible = String::new();
    let mut visible_width = 0usize;
    for character in input[start..].chars() {
        let character_width = unicode_width::UnicodeWidthChar::width(character).unwrap_or(0);
        if visible_width.saturating_add(character_width) > width {
            break;
        }
        visible.push(character);
        visible_width = visible_width.saturating_add(character_width);
    }
    (visible, cursor_column.min(width - 1))
}

#[cfg(test)]
mod tests {
    use super::input_viewport;

    #[test]
    fn input_viewport_starts_on_display_character_boundaries() {
        assert_eq!(input_viewport("abcdef", 6, 4), ("def".to_string(), 3));
        assert_eq!(input_viewport("你你好", 9, 4), ("好".to_string(), 2));
    }

    #[test]
    fn input_viewport_keeps_combining_marks_with_their_base() {
        assert_eq!(
            input_viewport("e\u{301}x", "e\u{301}".len(), 4),
            ("e\u{301}x".to_string(), 1)
        );
    }
}
