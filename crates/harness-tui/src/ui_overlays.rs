use super::*;

#[path = "ui_overlays/auth_dialog.rs"]
mod auth_dialog;
#[path = "ui_overlays/model_switcher.rs"]
mod model_switcher;
#[path = "ui_overlays/permission_modal.rs"]
mod permission_modal;
#[path = "ui_overlays/prompt_stash_dialog.rs"]
mod prompt_stash_dialog;
#[path = "ui_overlays/session_history.rs"]
mod session_history;
#[path = "ui_overlays/status_dialog.rs"]
mod status_dialog;
#[path = "ui_overlays/theme_dialog.rs"]
mod theme_dialog;
#[path = "ui_overlays/toggles_menu.rs"]
mod toggles_menu;

use auth_dialog::render_auth_dialog_overlay;
use model_switcher::{
    model_switcher_overlay_title, paint_model_select_panel, render_model_switcher_overlay,
};
pub(super) use permission_modal::{
    permission_modal_actions_text, permission_modal_draft_line, permission_modal_guidance,
    permission_modal_icon, permission_modal_metadata_line, permission_modal_subject_line,
    permission_modal_summary_line, permission_modal_title, question_permission_actions_text,
    question_permission_body_text,
};
use prompt_stash_dialog::render_prompt_stash_list_overlay;
use session_history::{
    render_fork_selector_input, render_fork_selector_list, render_lineage_browser_overlay,
    render_session_history_overlay, render_session_rename_dialog, session_history_overlay_title,
};
use status_dialog::render_status_dialog_overlay;
#[cfg(test)]
pub(crate) use status_dialog::{
    exact_test_status_dialog_formatters_section_disabled_when_none,
    exact_test_status_dialog_formatters_section_lists_enabled_language,
    exact_test_status_dialog_mcp_rows_match_harness_states,
    exact_test_status_dialog_render_snapshot_covers_harness_sections,
};
use theme_dialog::render_theme_dialog_overlay;
use toggles_menu::{render_toggles_menu_list, render_yolo_warning_popup};

pub(super) fn render_overlays(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    for overlay in &app.overlay_stack() {
        match overlay {
            OverlayKind::DetailsDrawer => {}
            OverlayKind::SlashCommands => {
                render_slash_commands_overlay(frame, app, theme, plan.slash_overlay)
            }
            OverlayKind::FileMentions => {
                render_file_mentions_overlay(frame, app, theme, plan.slash_overlay)
            }
            OverlayKind::CommandPalette => {
                render_command_palette_overlay(frame, app, theme, plan.root, plan.palette_overlay)
            }
            OverlayKind::TogglesMenu | OverlayKind::LineageBrowser | OverlayKind::ForkSelector => {
                render_command_palette_overlay(frame, app, theme, plan.root, plan.palette_overlay)
            }
            OverlayKind::StatusDialog => render_status_dialog_overlay(frame, app, theme, plan.root),
            OverlayKind::ThemeDialog => render_theme_dialog_overlay(frame, app, theme, plan.root),
            OverlayKind::PermissionModal => {}
            OverlayKind::ErrorDetails => render_error_details_overlay(frame, app, theme, plan.root),
            OverlayKind::PromptStashList => {
                render_prompt_stash_list_overlay(frame, app, theme, plan.root)
            }
            OverlayKind::AuthDialog => render_auth_dialog_overlay(frame, app, theme, plan.root),
        }
    }
}

fn render_command_palette_overlay(
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

    let title = if app.session_history_visible {
        session_history_overlay_title(app)
    } else if app.model_switcher_visible {
        model_switcher_overlay_title(app)
    } else if app.toggles_menu_visible {
        "Toggles".to_string()
    } else if app.lineage_browser_visible {
        "Harness session tree".to_string()
    } else if app.fork_selector_visible {
        "Fork session".to_string()
    } else {
        "Commands".to_string()
    };

    if app.session_history_visible {
        if !paint_command_palette_panel(frame, theme, overlay) {
            return;
        }
        render_session_history_overlay(frame, app, theme, overlay, &title);
        if app.session_rename_visible {
            render_session_rename_dialog(frame, app, theme, overlay);
        }
    } else if app.model_switcher_visible {
        if !paint_model_select_panel(frame, theme, overlay) {
            return;
        }
        render_model_switcher_overlay(frame, app, theme, overlay, &title);
    } else if app.toggles_menu_visible {
        if !paint_command_palette_panel(frame, theme, overlay) {
            return;
        }
        let Some((header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_header(frame, theme, header, &title);
        render_command_palette_input(frame, app, theme, input);
        render_toggles_menu_list(frame, app, theme, list);
        if app.toggles_yolo_confirmation_visible() {
            render_yolo_warning_popup(frame, theme, overlay);
        }
    } else if app.lineage_browser_visible {
        let Some(inner) = render_command_palette_surface(frame, theme, overlay) else {
            return;
        };
        render_lineage_browser_overlay(frame, app, theme, inner, &title);
    } else if app.fork_selector_visible {
        if !paint_command_palette_panel(frame, theme, overlay) {
            return;
        }
        let Some((header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_header(frame, theme, header, &title);
        render_fork_selector_input(frame, app, theme, input);
        render_fork_selector_list(frame, app, theme, list);
    } else {
        if !paint_command_palette_panel(frame, theme, overlay) {
            return;
        }
        let Some((header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_header(frame, theme, header, &title);
        render_command_palette_input(frame, app, theme, input);
        render_command_palette_list(frame, app, theme, list);
    }
}

fn render_slash_commands_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };
    if overlay.width <= 2 || overlay.height == 0 {
        return;
    }

    frame.render_widget(Clear, overlay);
    let inner = crate::layout::slash_command_overlay_content_area(overlay);
    render_slash_commands_list(frame, app, theme, inner);
}

fn render_file_mentions_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };
    if overlay.width <= 2 || overlay.height == 0 {
        return;
    }

    frame.render_widget(Clear, overlay);
    let inner = crate::layout::completion_overlay_content_area(overlay);
    render_file_mentions_list(frame, app, theme, inner);
}

fn render_file_mentions_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::slash_command_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if app.file_mention_entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().bg(surface)),
                Span::styled(
                    "No matching items",
                    Style::default()
                        .fg(ui_chrome::command_palette_muted(theme))
                        .bg(surface),
                ),
                Span::styled(" ", Style::default().bg(surface)),
            ])),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height);
    let selected = app
        .file_mention_selected
        .min(app.file_mention_entries.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    for (row, entry) in app
        .file_mention_entries
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        let is_selected = row == selected;
        frame.render_widget(
            Block::default().style(ui_chrome::slash_command_row_style(theme, is_selected)),
            row_area,
        );
        frame.render_widget(
            Paragraph::new(file_mention_row(entry, is_selected, theme, row_area.width)),
            row_area,
        );
    }
}

fn file_mention_row(
    entry: &crate::app::FileMentionEntry,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let row_style = ui_chrome::slash_command_row_style(theme, is_selected);
    let label_style = if is_selected {
        row_style.fg(ui_chrome::slash_command_selection_fg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let side_padding = usize::from(row_width > 0);
    let available_width = row_width.saturating_sub(side_padding.saturating_mul(2));
    let label = truncate_plain_text(&entry.display, available_width);
    let consumed = side_padding.saturating_add(label.chars().count());
    let trailing = row_width.saturating_sub(consumed);

    let mut spans = Vec::new();
    if side_padding > 0 {
        spans.push(Span::styled(" ".repeat(side_padding), row_style));
    }
    if !label.is_empty() {
        spans.push(Span::styled(label, label_style));
    }
    if trailing > 0 {
        spans.push(Span::styled(" ".repeat(trailing), row_style));
    }
    Line::from(spans)
}

fn render_slash_commands_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::slash_command_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if app.slash_filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().bg(surface)),
                Span::styled(
                    "No matching items",
                    Style::default()
                        .fg(ui_chrome::command_palette_muted(theme))
                        .bg(surface),
                ),
                Span::styled(" ", Style::default().bg(surface)),
            ])),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height);
    let selected = app
        .slash_selected
        .min(app.slash_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));
    let command_column_width = app.slash_command_column_width();
    for (row, command) in app
        .slash_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        let is_selected = row == selected;
        frame.render_widget(
            Block::default().style(ui_chrome::slash_command_row_style(theme, is_selected)),
            row_area,
        );

        frame.render_widget(
            Paragraph::new(slash_command_row(
                command,
                crate::keybindings::slash_command_description(command),
                is_selected,
                theme,
                row_area.width,
                command_column_width,
            )),
            row_area,
        );
    }
}

fn slash_command_row(
    command: &str,
    description: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
    command_column_width: usize,
) -> Line<'static> {
    let row_width = usize::from(width);
    let row_style = ui_chrome::slash_command_row_style(theme, is_selected);
    let label_style = if is_selected {
        row_style.fg(ui_chrome::slash_command_selection_fg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if is_selected {
        row_style.fg(ui_chrome::slash_command_selection_fg(theme))
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };

    let label = slash_command_display(command);
    let side_padding = usize::from(row_width > 0);
    let available_width = row_width.saturating_sub(side_padding.saturating_mul(2));
    let label_width = label.chars().count();
    let label_column_width = command_column_width.max(label_width).min(available_width);
    let label = truncate_plain_text(&label, label_column_width);
    let label_used = label.chars().count();
    let label_padding = label_column_width.saturating_sub(label_used);
    let description_width = available_width.saturating_sub(label_column_width);
    let description = truncate_plain_text(description, description_width);
    let consumed = side_padding
        .saturating_add(label_used)
        .saturating_add(label_padding)
        .saturating_add(description.chars().count());
    let trailing = row_width.saturating_sub(consumed);

    let mut spans = Vec::new();
    if side_padding > 0 {
        spans.push(Span::styled(" ".repeat(side_padding), row_style));
    }
    if !label.is_empty() {
        spans.push(Span::styled(label, label_style));
    }
    if label_padding > 0 {
        spans.push(Span::styled(" ".repeat(label_padding), row_style));
    }
    if !description.is_empty() && description_width > 0 {
        spans.push(Span::styled(description, description_style));
    }
    if trailing > 0 {
        spans.push(Span::styled(" ".repeat(trailing), row_style));
    }

    Line::from(spans)
}

fn slash_command_display(command: &str) -> String {
    format!("/{command}")
}

fn render_command_palette_surface(frame: &mut Frame, theme: &Theme, overlay: Rect) -> Option<Rect> {
    if !paint_command_palette_panel(frame, theme, overlay) {
        return None;
    }

    let content = inset_rect(overlay, 3.min(overlay.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return None;
    }

    Some(content)
}

fn paint_command_palette_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
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

fn command_palette_dialog_layout(overlay: Rect) -> Option<(Rect, Rect, Rect)> {
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
    Some((header, input, list))
}

fn render_command_palette_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
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

fn render_palette_empty_message(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let top_padding = if area.height > 1 { 1 } else { 0 };
    let empty_area = Rect::new(
        area.x.saturating_add(3),
        area.y.saturating_add(top_padding),
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

fn render_command_palette_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let line = if app.palette_input.is_empty() {
        let placeholder = if app.session_history_visible {
            "Search"
        } else if app.model_switcher_visible {
            "Filter models, providers"
        } else if app.toggles_menu_visible {
            "Filter toggles"
        } else if app.lineage_browser_visible {
            "Filter Harness session tree"
        } else {
            "Search"
        };
        Line::from(vec![
            Span::styled(
                "█",
                Style::default()
                    .fg(command_palette_input_cursor(theme, app))
                    .bg(surface),
            ),
            Span::styled(
                format!(" {placeholder}"),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
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
            Span::styled(
                before.to_string(),
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(surface),
            ),
            Span::styled(
                "█",
                Style::default()
                    .fg(command_palette_input_cursor(theme, app))
                    .bg(surface),
            ),
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

fn command_palette_input_cursor(theme: &Theme, app: &AppState) -> Color {
    if app.session_history_visible {
        ui_chrome::fork_selector_cursor()
    } else {
        ui_chrome::command_palette_cursor(theme)
    }
}

fn render_command_palette_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let list_area = inset_rect(area, 1.min(area.width.saturating_sub(1)), 0);
    if list_area.width == 0 || list_area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
        list_area,
    );

    if app.palette_filtered.is_empty() {
        render_palette_empty_message(frame, theme, list_area, "No results found");
        return;
    }

    let visible_rows = usize::from(list_area.height);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let rows = palette_overlay_rows(app);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, PaletteOverlayRow::Command { is_selected, .. } if *is_selected == selected))
        .unwrap_or(0);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

    for (row, palette_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(list_area.x, row_y, list_area.width, 1);
        match palette_row {
            PaletteOverlayRow::Spacer => {
                frame.render_widget(
                    Block::default()
                        .style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
                    row_area,
                );
            }
            PaletteOverlayRow::Section(category) => {
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(
                        category.label(),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
            PaletteOverlayRow::Command {
                title,
                description,
                footer,
                is_selected,
            } => {
                let is_selected = *is_selected == selected;
                if is_selected {
                    frame.render_widget(
                        Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                        row_area,
                    );
                }

                frame.render_widget(
                    Paragraph::new(command_palette_row(
                        title,
                        description,
                        footer,
                        is_selected,
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
        }
    }
}

pub(crate) enum PaletteOverlayRow {
    Spacer,
    Section(crate::keybindings::palette_model::PaletteCategory),
    Command {
        title: String,
        description: String,
        footer: String,
        is_selected: usize,
    },
}

pub(crate) fn palette_overlay_rows(app: &AppState) -> Vec<PaletteOverlayRow> {
    use crate::app::palette_controller::compute_palette_rows;
    use crate::keybindings::palette_model::{find, PaletteDispatch};

    let rows = compute_palette_rows(app, &app.palette_input);
    let mut overlay_rows = Vec::new();
    let mut last_category: Option<crate::keybindings::palette_model::PaletteCategory> = None;

    for (selected_index, row) in rows.iter().enumerate() {
        if Some(row.category) != last_category {
            if last_category.is_some() {
                overlay_rows.push(PaletteOverlayRow::Spacer);
            }
            overlay_rows.push(PaletteOverlayRow::Section(row.category));
            last_category = Some(row.category);
        }

        let footer = {
            let entry = find(row.command_id);
            entry
                .and_then(|e| match e.dispatch {
                    PaletteDispatch::Action(action) => Some(app.keymap.get_binding_str(action)),
                    PaletteDispatch::OpenModelSwitcher => {
                        Some(app.keymap.get_binding_str(Action::OpenModelSwitcher))
                    }
                    _ => None,
                })
                .filter(|s| s != "-")
                .unwrap_or_default()
        };

        overlay_rows.push(PaletteOverlayRow::Command {
            title: row.title.clone(),
            description: row.description.to_string(),
            footer,
            is_selected: selected_index,
        });
    }

    overlay_rows
}

fn command_palette_row(
    label: &str,
    description: &str,
    shortcut: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(surface)
    };
    let label_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if is_selected {
        row_style
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let shortcut_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };

    let content_width = row_width.saturating_sub(3);
    let reserved_shortcut = if shortcut.is_empty() {
        0
    } else {
        shortcut.chars().count().saturating_add(1)
    };
    let body_width = content_width.saturating_sub(reserved_shortcut);
    let prefix = "   ";
    let mut spans = vec![Span::styled(prefix.to_string(), row_style)];
    let mut used_width = prefix.chars().count();

    let label = truncate_plain_text(label, 61usize.min(body_width.saturating_sub(used_width)));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(label, label_style));

    let gap_width = 1;
    let available_description = body_width.saturating_sub(used_width.saturating_add(gap_width));
    let description = truncate_plain_text(description, available_description);
    if !description.is_empty() {
        spans.push(Span::styled(" ", row_style));
        used_width = used_width.saturating_add(gap_width);
        used_width = used_width.saturating_add(description.chars().count());
        spans.push(Span::styled(description, description_style));
    }

    if used_width < body_width {
        spans.push(Span::styled(" ".repeat(body_width - used_width), row_style));
    }

    if !shortcut.is_empty() {
        spans.push(Span::styled(" ", row_style));
        spans.push(Span::styled(shortcut.to_string(), shortcut_style));
    }

    if content_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - content_width),
            row_style,
        ));
    }

    Line::from(spans)
}

fn command_palette_section_row(label: &str, theme: &Theme, width: u16) -> Line<'static> {
    let row_width = usize::from(width);
    let surface = ui_chrome::command_palette_surface(theme);
    let prefix = "   ";
    let mut spans = vec![Span::styled(prefix, Style::default().bg(surface))];
    let label = truncate_plain_text(label, row_width.saturating_sub(prefix.chars().count()));
    let label_width = label.chars().count();
    spans.push(Span::styled(
        label,
        Style::default()
            .fg(ui_chrome::command_palette_section())
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    ));
    let used_width = prefix.chars().count().saturating_add(label_width);
    if used_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - used_width),
            Style::default().bg(surface),
        ));
    }
    Line::from(spans)
}

fn render_overlay_dim_backdrop(frame: &mut Frame, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let buffer = frame.buffer_mut();
    let max_x = area.x.saturating_add(area.width);
    let max_y = area.y.saturating_add(area.height);
    for y in area.y..max_y {
        for x in area.x..max_x {
            let cell = &mut buffer[(x, y)];
            cell.set_fg(dim_overlay_color(cell.fg));
            cell.set_bg(dim_overlay_color(cell.bg));
        }
    }
}

fn dim_overlay_color(color: Color) -> Color {
    let Some((red, green, blue)) = color_rgb(color) else {
        return color;
    };
    Color::Rgb(
        scrim_channel(red),
        scrim_channel(green),
        scrim_channel(blue),
    )
}

fn scrim_channel(channel: u8) -> u8 {
    let channel = u16::from(channel);
    u8::try_from(channel.saturating_mul(105) / 255).unwrap_or_default()
}

fn color_rgb(color: Color) -> Option<(u8, u8, u8)> {
    match color {
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(red, green, blue) => Some((red, green, blue)),
        Color::Indexed(index) => Some((index, index, index)),
        Color::Reset => None,
    }
}

fn render_error_details_overlay(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    if root.width == 0 || root.height == 0 {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let overlay_width = root.width.clamp(40, 80);
    let overlay_height = root.height.clamp(8, 20);
    let overlay_x = root.x + (root.width.saturating_sub(overlay_width)) / 2;
    let overlay_y = root.y + (root.height.saturating_sub(overlay_height)) / 2;
    let overlay = Rect::new(overlay_x, overlay_y, overlay_width, overlay_height);

    let surface = ui_chrome::elevated_card_surface(theme);
    let border = theme.status.error;
    let title_color = theme.status.error;

    let block = ui_chrome::interruptive_modal_block(
        theme,
        Line::from("Error details"),
        border,
        title_color,
        ui_chrome::ChromeFrame::Frame,
    );
    let inner = block.inner(overlay);
    frame.render_widget(Clear, overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let activity = app
        .activities
        .get(app.transcript_view.selected_activity_index);
    let error_text = activity
        .and_then(|a| a.error_message.as_deref())
        .unwrap_or("No error details available");

    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let error_style = Style::default().fg(theme.status.error).bg(surface);

    let mut lines = Vec::new();
    lines.push(Line::from(vec![Span::styled("Error:", error_style)]));
    lines.push(Line::default());
    for line in error_text.lines() {
        lines.push(Line::from(vec![Span::styled(
            truncate_plain_text(line, usize::from(inner.width)),
            primary_style,
        )]));
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![Span::styled(
        "esc close  ·  r resubmit",
        muted_style,
    )]));

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(surface))
            .wrap(Wrap { trim: true }),
        inner,
    );
}
