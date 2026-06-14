use super::*;

pub(super) fn render_child_sessions_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };
    render_overlay_dim_backdrop(frame, root);
    if !select_dialog::paint_select_dialog_panel(frame, theme, overlay) {
        return;
    }
    let Some(layout) = select_dialog::select_dialog_layout(overlay) else {
        return;
    };
    select_dialog::render_select_dialog_header(frame, theme, layout.header, "Child sessions");
    select_dialog::render_select_dialog_input(
        frame,
        theme,
        layout.input,
        "",
        0,
        "Child sessions",
        ui_chrome::fork_selector_cursor(),
    );
    render_child_session_rows(frame, app, theme, layout.list);
}

fn render_child_session_rows(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let view = app.child_session_dialog_view_model();
    if let Some(message) = view.empty_message.as_deref() {
        select_dialog::render_select_dialog_empty_message(frame, theme, area, message);
        return;
    }
    for (row_index, row) in view.rows.iter().take(usize::from(area.height)).enumerate() {
        let row_area = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::try_from(row_index).unwrap_or(u16::MAX)),
            area.width,
            1,
        );
        let row_style = child_session_row_style(theme, row.selected);
        frame.render_widget(Block::default().style(row_style), row_area);
        let request = row
            .request_id
            .as_deref()
            .map(|request_id| format!(" · {request_id}"))
            .unwrap_or_default();
        let label = format!(
            "{}{} · {} · {}",
            row.session_id, request, row.status, row.title
        );
        frame.render_widget(
            Paragraph::new(truncate_plain_text(&label, usize::from(row_area.width)))
                .style(row_style.fg(ui_chrome::command_palette_title(theme))),
            row_area,
        );
    }
}

fn child_session_row_style(theme: &Theme, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(ui_chrome::fork_selector_selection_fg(theme))
            .bg(ui_chrome::fork_selector_selection_bg())
    } else {
        Style::default()
            .fg(ui_chrome::command_palette_title(theme))
            .bg(ui_chrome::command_palette_surface(theme))
    }
}
