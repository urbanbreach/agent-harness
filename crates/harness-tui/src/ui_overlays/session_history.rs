use harness_core::proj::RunStatus;

use super::*;
use crate::app::session_navigation::{
    session_history_category_label, session_history_current_marker, session_history_display_title,
    session_history_footer_label,
};
use crate::time_format::short_time_or_trimmed;

pub(super) fn render_session_history_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Rect,
    title: &str,
) {
    if overlay.width <= 8 || overlay.height <= 5 {
        return;
    }

    let content_x = overlay.x.saturating_add(4);
    let content_width = overlay.width.saturating_sub(8);
    let header = Rect::new(content_x, overlay.y.saturating_add(1), content_width, 1);
    let input = Rect::new(content_x, overlay.y.saturating_add(3), content_width, 1);
    let actions = Rect::new(
        content_x,
        overlay.y.saturating_add(overlay.height.saturating_sub(2)),
        content_width,
        1,
    );
    let list_y = overlay.y.saturating_add(5);
    let list_bottom = actions.y.saturating_sub(1);
    let list = Rect::new(
        overlay.x.saturating_add(1),
        list_y,
        overlay.width.saturating_sub(2),
        list_bottom.saturating_sub(list_y),
    );

    render_command_palette_header(frame, theme, header, title);
    render_command_palette_input(frame, app, theme, input);
    render_session_history_list(frame, app, theme, list);
    render_session_history_actions(frame, theme, actions);
}

pub(super) fn render_lineage_browser_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    title: &str,
) {
    let child_dialog = app.lineage_child_dialog_view_model();
    let dialog_height: u16 = if child_dialog.is_some() { 3 } else { 0 };
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(dialog_height),
        ])
        .split(area);
    let surface = ui_chrome::command_palette_surface(theme);

    render_command_palette_header(frame, theme, sections[0], title);
    render_command_palette_input(frame, app, theme, sections[1]);
    frame.render_widget(
        Paragraph::new("Read-only · type to filter · Space folds · Enter keeps selection")
            .style(Style::default().fg(theme.text.secondary).bg(surface)),
        sections[2],
    );
    render_lineage_browser_list(frame, app, theme, sections[3]);
    if let Some(dialog) = child_dialog {
        render_lineage_child_dialog(frame, theme, sections[4], &dialog);
    }
}

fn render_lineage_browser_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let list_area = inset_rect(area, 1.min(area.width.saturating_sub(1)), 0);
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        list_area,
    );

    let vm = app.lineage_browser_view_model();
    if let Some(message) = vm.empty_message {
        render_palette_empty_message(frame, theme, list_area, &message);
        return;
    }

    let selected = vm.rows.iter().position(|row| row.selected).unwrap_or(0);
    let visible_rows = usize::from(list_area.height);
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    for (row_index, row) in vm.rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_area = Rect::new(
            list_area.x,
            list_area
                .y
                .saturating_add(u16::try_from(row_index - scroll).unwrap_or(u16::MAX)),
            list_area.width,
            1,
        );
        frame.render_widget(
            Block::default().style(lineage_row_style(theme, row.selected)),
            row_area,
        );
        frame.render_widget(
            Paragraph::new(lineage_browser_row(row, theme, row_area.width)),
            row_area,
        );
    }
}

fn render_lineage_child_dialog(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dialog: &crate::view_model::LineageChildDialogViewModel,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let border_style = Style::default().fg(theme.border.subtle).bg(surface);
    let label_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let meta_style = Style::default().fg(theme.text.secondary).bg(surface);
    let key_style = Style::default().fg(theme.text.primary).bg(surface);

    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(border_style)
            .style(Style::default().bg(surface)),
        area,
    );

    let content_area = Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(1),
        area.width.saturating_sub(4),
        1,
    );
    if content_area.width == 0 {
        return;
    }

    let index_total = if dialog.child_total > 0 {
        format!(" ({} of {})", dialog.child_index, dialog.child_total)
    } else {
        String::new()
    };
    let meta = format!("{}{}", dialog.label, index_total);
    let nav = format!(
        "First {}  Prev {}  Next {}  Parent {}",
        dialog.first_child_shortcut,
        dialog.previous_shortcut,
        dialog.next_shortcut,
        dialog.parent_shortcut,
    );

    let content_width = usize::from(content_area.width);
    let nav_width = nav.chars().count();
    let meta_width = meta.chars().count();

    if nav_width + 2 + meta_width.min(content_width.saturating_sub(nav_width).saturating_sub(2))
        <= content_width
    {
        let max_meta = content_width.saturating_sub(nav_width).saturating_sub(2);
        let meta_text = truncate_plain_text(&meta, max_meta);
        let used = meta_text.chars().count();
        let gap = content_width.saturating_sub(used).saturating_sub(nav_width);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(meta_text, meta_style),
                Span::styled(" ".repeat(gap), meta_style),
                Span::styled(nav, key_style),
            ])),
            content_area,
        );
    } else {
        let title = truncate_plain_text(&dialog.title, content_width.min(meta_width.max(1)));
        let used = title.chars().count();
        let gap = content_width.saturating_sub(used).saturating_sub(nav_width);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(title, label_style),
                Span::styled(" ".repeat(gap), meta_style),
                Span::styled(nav, key_style),
            ])),
            content_area,
        );
    }
}

pub(super) fn render_fork_selector_list(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
) {
    let list_area = inset_rect(area, 1.min(area.width.saturating_sub(1)), 0);
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        list_area,
    );

    let vm = app.fork_selector_view_model();
    if let Some(message) = vm.empty_message {
        render_fork_selector_empty_message(frame, theme, area, &message);
        return;
    }

    let selected = vm.rows.iter().position(|row| row.selected).unwrap_or(0);
    let visible_rows = usize::from(list_area.height);
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    for (row_index, row) in vm.rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_area = Rect::new(
            list_area.x,
            list_area
                .y
                .saturating_add(u16::try_from(row_index - scroll).unwrap_or(u16::MAX)),
            list_area.width,
            1,
        );
        frame.render_widget(
            Block::default().style(lineage_row_style(theme, row.selected)),
            row_area,
        );
        frame.render_widget(
            Paragraph::new(fork_selector_row(row, theme, row_area.width)),
            row_area,
        );
    }
}

fn render_fork_selector_empty_message(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
    if area.width <= 8 || area.height <= 1 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let empty_area = Rect::new(
        area.x.saturating_add(4),
        area.y.saturating_add(1),
        area.width.saturating_sub(8),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_plain_text(message, usize::from(empty_area.width)),
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(surface),
        ))),
        empty_area,
    );
}

fn lineage_browser_row(
    row: &crate::view_model::LineageBrowserRowViewModel,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_style = lineage_row_style(theme, row.selected);
    let selected_fg = ui_chrome::slash_command_selection_fg(theme);
    let title_style = if row.selected {
        row_style.fg(selected_fg).add_modifier(Modifier::BOLD)
    } else if row.current {
        row_style.fg(theme.status.info).add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let meta_style = if row.selected {
        row_style.fg(selected_fg)
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let fold = if row.child_count == 0 {
        "•"
    } else if row.expanded {
        "▾"
    } else {
        "▸"
    };
    let indent = "  ".repeat(row.depth.min(8));
    let status = row.status.map(run_status_label).unwrap_or("unknown");
    let current = if row.current { " · current" } else { "" };
    let meta = format!(
        "{status}{current} · {} child{}",
        row.child_count,
        plural_s(row.child_count)
    );
    split_title_meta_row(
        format!(" {indent}{fold} {}", row.title),
        meta,
        title_style,
        meta_style,
        row_style,
        width,
    )
}

fn fork_selector_row(
    row: &crate::view_model::ForkSelectorRowViewModel,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    const TITLE_PADDING: usize = 6;
    const RIGHT_PADDING: usize = 3;
    const MAX_TITLE_WIDTH: usize = 61;

    let row_style = fork_selector_row_style(theme, row.selected);
    let selected_fg = ui_chrome::fork_selector_selection_fg(theme);
    let title_style = if row.selected {
        row_style.fg(selected_fg).add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let meta_style = if row.selected {
        row_style.fg(selected_fg)
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let status = row.status.map(run_status_label).unwrap_or("stable");
    let meta = if row.event_id.is_none() {
        String::new()
    } else {
        row.timestamp
            .as_deref()
            .map(short_time_or_trimmed)
            .unwrap_or_else(|| status.to_string())
    };

    let row_width = usize::from(width);
    let content_width = row_width.saturating_sub(RIGHT_PADDING);
    let meta_width = meta.chars().count().min(content_width / 2);
    let title_width = content_width
        .saturating_sub(TITLE_PADDING)
        .saturating_sub(meta_width)
        .saturating_sub(usize::from(meta_width > 0));
    let title = truncate_plain_text(
        &row.prompt_text.replace('\n', " "),
        title_width.min(MAX_TITLE_WIDTH),
    );
    let title_used = title.chars().count();
    let gap = content_width
        .saturating_sub(TITLE_PADDING)
        .saturating_sub(title_used)
        .saturating_sub(meta_width);
    let meta = truncate_plain_text(&meta, meta_width);

    Line::from(vec![
        Span::styled(" ".repeat(TITLE_PADDING), row_style),
        Span::styled(title, title_style),
        Span::styled(" ".repeat(gap), row_style),
        Span::styled(meta, meta_style),
        Span::styled(" ".repeat(RIGHT_PADDING), row_style),
    ])
}

fn split_title_meta_row(
    title: String,
    meta: String,
    title_style: Style,
    meta_style: Style,
    row_style: Style,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let meta_width = meta.chars().count().min(row_width / 2);
    let title_width = row_width.saturating_sub(meta_width).saturating_sub(1);
    let title = truncate_plain_text(&title, title_width);
    let used = title.chars().count();
    let gap = row_width.saturating_sub(used).saturating_sub(meta_width);
    let meta = truncate_plain_text(&meta, meta_width);
    Line::from(vec![
        Span::styled(title, title_style),
        Span::styled(" ".repeat(gap), row_style),
        Span::styled(meta, meta_style),
    ])
}

fn lineage_row_style(theme: &Theme, selected: bool) -> Style {
    if selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(ui_chrome::command_palette_surface(theme))
    }
}

fn fork_selector_row_style(theme: &Theme, selected: bool) -> Style {
    let surface = ui_chrome::command_palette_surface(theme);
    if selected {
        Style::default().bg(ui_chrome::fork_selector_selection_bg())
    } else {
        Style::default().bg(surface)
    }
}

const fn run_status_label(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Running => "running",
        RunStatus::Finished => "finished",
        RunStatus::Failed => "failed",
    }
}

const fn plural_s(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "ren"
    }
}

pub(super) fn render_fork_selector_input(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let input_style = Style::default()
        .fg(ui_chrome::command_palette_muted(theme))
        .bg(surface);
    let cursor_style = Style::default()
        .fg(ui_chrome::fork_selector_cursor())
        .bg(surface);
    let line = if app.palette_input.is_empty() {
        Line::from(vec![
            Span::styled("█", cursor_style),
            Span::styled(" Search", input_style),
        ])
    } else {
        let cursor_byte = app
            .palette_input
            .char_indices()
            .nth(app.palette_cursor)
            .map(|(index, _)| index)
            .unwrap_or(app.palette_input.len());
        let before = &app.palette_input[..cursor_byte];
        let after = &app.palette_input[cursor_byte..];
        Line::from(vec![
            Span::styled(before.to_string(), input_style),
            Span::styled("█", cursor_style),
            Span::styled(after.to_string(), input_style),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_session_history_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.session_history_filtered.is_empty() {
        render_palette_empty_message(frame, theme, area, "No results found");
        return;
    }

    let rows = session_history_visual_rows(app);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, SessionHistoryVisualRow::Entry { selected: true, .. }))
        .unwrap_or(0);
    let visible_rows = usize::from(area.height).max(1);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));
    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    for (row_index, row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_area = Rect::new(
            area.x,
            area.y
                .saturating_add(u16::try_from(row_index - scroll).unwrap_or(u16::MAX)),
            area.width,
            1,
        );
        match row {
            SessionHistoryVisualRow::Gap => {}
            SessionHistoryVisualRow::Header(label) => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        truncate_plain_text(label, usize::from(row_area.width.saturating_sub(3))),
                        Style::default()
                            .fg(ui_chrome::command_palette_section())
                            .bg(surface)
                            .add_modifier(Modifier::BOLD),
                    )))
                    .style(Style::default().bg(surface)),
                    Rect::new(
                        row_area.x.saturating_add(3),
                        row_area.y,
                        row_area.width.saturating_sub(3),
                        1,
                    ),
                );
            }
            SessionHistoryVisualRow::Entry {
                entry_index,
                selected,
            } => {
                let Some(entry) = app.session_history_entries.get(*entry_index) else {
                    continue;
                };
                let row_style = session_history_row_style(theme, *selected);
                frame.render_widget(Block::default().style(row_style), row_area);
                frame.render_widget(
                    Paragraph::new(session_history_row(
                        entry,
                        app,
                        *selected,
                        theme,
                        row_area.width,
                    ))
                    .style(row_style),
                    row_area,
                );
            }
        }
    }
}

pub(super) fn session_history_overlay_title(app: &AppState) -> String {
    match app.startup_launcher_action {
        crate::app::StartupLauncherAction::ContinueSession => "Continue session".to_string(),
        crate::app::StartupLauncherAction::ReplaySession => "Replay session".to_string(),
        crate::app::StartupLauncherAction::NewSession => "Sessions".to_string(),
    }
}

enum SessionHistoryVisualRow {
    Gap,
    Header(String),
    Entry { entry_index: usize, selected: bool },
}

fn session_history_visual_rows(app: &AppState) -> Vec<SessionHistoryVisualRow> {
    let mut rows = Vec::new();
    let mut previous_category: Option<String> = None;
    let selected = app
        .session_history_selected
        .min(app.session_history_filtered.len().saturating_sub(1));
    for (filtered_index, entry_index) in app.session_history_filtered.iter().enumerate() {
        let Some(entry) = app.session_history_entries.get(*entry_index) else {
            continue;
        };
        let is_pinned = app.session_pins.contains(&entry.catalog.run_id);
        let category = if is_pinned {
            "Pinned".to_string()
        } else {
            session_history_category_label(entry)
        };
        if previous_category.as_deref() != Some(category.as_str()) {
            if previous_category.is_some() {
                rows.push(SessionHistoryVisualRow::Gap);
            }
            rows.push(SessionHistoryVisualRow::Header(category.clone()));
            previous_category = Some(category);
        }
        rows.push(SessionHistoryVisualRow::Entry {
            entry_index: *entry_index,
            selected: filtered_index == selected,
        });
    }
    rows
}

fn session_history_row(
    entry: &crate::app::SessionHistoryEntry,
    app: &AppState,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let current = session_history_current_marker(entry, app.current_session_id());
    let is_pinned = app.session_pins.contains(&entry.catalog.run_id);
    let is_armed = app
        .session_delete_armed_run_id
        .as_deref()
        .is_some_and(|armed| armed == entry.catalog.run_id);
    let row_style = session_history_row_style(theme, is_selected);
    let text_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else if current {
        Style::default().fg(ui_chrome::fork_selector_cursor())
    } else {
        Style::default().fg(theme.text.primary)
    };
    let footer_style = if is_armed {
        Style::default().fg(theme.status.warning)
    } else if is_selected {
        row_style
    } else {
        Style::default().fg(theme.text.secondary)
    };
    let marker_style = if is_selected {
        row_style
    } else {
        Style::default().fg(ui_chrome::fork_selector_cursor())
    };
    let pin_marker = if is_pinned { "📌 " } else { "" };
    let left_padding = if current { 1usize } else { 3usize };
    let marker = if current { "●" } else { "" };
    let marker_gap = usize::from(current);
    let footer = if is_armed {
        "Press ctrl+d again to confirm".to_string()
    } else {
        match app.startup_launcher_action {
            crate::app::StartupLauncherAction::ContinueSession if !entry.catalog.is_resumable => {
                entry
                    .catalog
                    .resume_disabled_reason
                    .clone()
                    .unwrap_or_else(|| "continue unavailable".to_string())
            }
            crate::app::StartupLauncherAction::ContinueSession => "continue ready".to_string(),
            crate::app::StartupLauncherAction::ReplaySession => "replay ready".to_string(),
            crate::app::StartupLauncherAction::NewSession => session_history_footer_label(entry),
        }
    };
    let footer_width = footer.chars().count();
    let title_padding = 3usize;
    let fixed_width = left_padding
        .saturating_add(marker.chars().count())
        .saturating_add(marker_gap)
        .saturating_add(title_padding)
        .saturating_add(pin_marker.chars().count())
        .saturating_add(footer_width);
    let title_width = row_width.saturating_sub(fixed_width).min(61);
    let display_title = session_history_display_title(entry);
    let title = truncate_plain_text(&display_title, title_width);
    let used_width = fixed_width.saturating_add(title.chars().count());
    let gap_width = row_width.saturating_sub(used_width);

    let mut spans = vec![Span::styled(" ".repeat(left_padding), row_style)];
    if current {
        spans.push(Span::styled(marker.to_string(), marker_style));
        spans.push(Span::styled(" ", row_style));
    }
    spans.push(Span::styled(" ".repeat(title_padding), row_style));
    spans.push(Span::styled(pin_marker.to_string(), row_style));
    spans.push(Span::styled(title, text_style));
    spans.push(Span::styled(" ".repeat(gap_width), row_style));
    if !footer.is_empty() {
        spans.push(Span::styled(footer, footer_style));
    }

    Line::from(spans)
}

fn session_history_row_style(theme: &Theme, selected: bool) -> Style {
    if selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(ui_chrome::command_palette_surface(theme))
    }
}

fn render_session_history_actions(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let action_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let key_style = Style::default().fg(theme.text.secondary).bg(surface);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("pin", action_style),
            Span::styled(" ctrl+f  ", key_style),
            Span::styled("delete", action_style),
            Span::styled(" ctrl+d  ", key_style),
            Span::styled("rename", action_style),
            Span::styled(" ctrl+r", key_style),
        ]))
        .style(Style::default().bg(surface)),
        area,
    );
}

pub(super) fn render_session_rename_dialog(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Rect,
) {
    let dialog_width = 50u16.min(overlay.width.saturating_sub(4));
    let dialog_height = 7u16.min(overlay.height.saturating_sub(4));
    let dialog_x = overlay.x + (overlay.width.saturating_sub(dialog_width)) / 2;
    let dialog_y = overlay.y + (overlay.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(dialog_x, dialog_y, dialog_width, dialog_height);

    let surface = ui_chrome::command_palette_surface(theme);
    let border_style = Style::default().fg(theme.border.strong).bg(surface);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .bg(surface)
        .add_modifier(Modifier::BOLD);
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
        Paragraph::new(Line::from(Span::styled(" Rename session", title_style))),
        title_area,
    );

    let input_y = dialog_y + 2;
    let input_area = Rect::new(dialog_x + 1, input_y, dialog_width.saturating_sub(2), 1);
    let cursor_byte = app
        .session_rename_input
        .char_indices()
        .nth(app.session_rename_cursor)
        .map(|(index, _)| index)
        .unwrap_or(app.session_rename_input.len());
    let before = &app.session_rename_input[..cursor_byte];
    let after = &app.session_rename_input[cursor_byte..];
    let input_style = Style::default().fg(theme.text.primary).bg(surface);
    let cursor_style = Style::default()
        .fg(ui_chrome::fork_selector_cursor())
        .bg(surface);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(before.to_string(), input_style),
            Span::styled("█", cursor_style),
            Span::styled(after.to_string(), input_style),
        ])),
        input_area,
    );

    let hint_y = input_y + 2;
    if hint_y < dialog_y + dialog_height {
        let hint_area = Rect::new(dialog_x + 1, hint_y, dialog_width.saturating_sub(2), 1);
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                " Enter to rename · Esc to cancel",
                muted_style,
            ))),
            hint_area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_model::ForkSelectorRowViewModel;

    #[test]
    fn fork_selector_row_matches_reference_dialog_select_padding_and_colors() {
        let theme = Theme::default();
        let row = ForkSelectorRowViewModel {
            cutoff_seq: 2,
            event_count: 2,
            run_id: Some("run".to_string()),
            status: None,
            event_id: Some("event".to_string()),
            event_kind: "UserMessageSubmitted",
            prompt_text: "Fork this prompt".to_string(),
            timestamp: Some("2026-05-04T12:34:56Z".to_string()),
            selected: true,
        };

        let line = fork_selector_row(&row, &theme, 86);

        assert_eq!(line.spans[0].content.as_ref(), "      ");
        assert_eq!(line.spans[0].style.bg, Some(Color::Rgb(0xFA, 0xB2, 0x83)));
        assert_eq!(line.spans[1].content.as_ref(), "Fork this prompt");
        assert_eq!(line.spans[1].style.fg, Some(theme.text.inverse));
        assert!(line.spans[1].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(line.spans[3].content.as_ref(), "12:34");
        assert_eq!(line.spans[3].style.fg, Some(theme.text.inverse));
        assert_eq!(line.spans[4].content.as_ref(), "   ");
    }
}
