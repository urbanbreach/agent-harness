use super::auth_dialog::PromptPanel;
use super::*;

pub(super) fn render_new_worktree_dialog(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    frame.render_widget(Clear, root);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.overlay)),
        root,
    );
    let input_area = PromptPanel {
        title: "Create worktree",
        description: Some("Name (optional). Leave blank to generate one."),
        placeholder: "Worktree name",
        value: &app.new_worktree_dialog.input,
        secret: false,
        error: None,
        footer: "enter create",
    }
    .render(frame, theme, root);
    let cursor_width = unicode_width::UnicodeWidthStr::width(
        &app.new_worktree_dialog.input[..app.new_worktree_dialog.cursor],
    );
    let cursor_x = input_area
        .x
        .saturating_add(u16::try_from(cursor_width).unwrap_or(u16::MAX))
        .min(input_area.right().saturating_sub(1));
    frame.set_cursor_position((cursor_x, input_area.y));
}
