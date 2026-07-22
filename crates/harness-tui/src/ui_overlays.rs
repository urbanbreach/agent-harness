// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use super::*;

#[path = "ui_overlays/auth_dialog.rs"]
mod auth_dialog;
#[path = "ui_overlays/model_switcher.rs"]
mod model_switcher;
#[path = "ui_overlays/permission_modal.rs"]
mod permission_modal;
#[path = "ui_overlays/plan_view.rs"]
mod plan_view;
#[path = "ui_overlays/prompt_stash_dialog.rs"]
mod prompt_stash_dialog;
#[path = "ui_overlays/session_history.rs"]
mod session_history;
#[path = "ui_overlays/settings_editor.rs"]
mod settings_editor;
#[path = "ui_overlays/status_dialog.rs"]
mod status_dialog;
#[path = "ui_overlays/theme_dialog.rs"]
mod theme_dialog;
#[path = "ui_overlays/toggles_menu.rs"]
mod toggles_menu;

use auth_dialog::render_auth_dialog_overlay;
use model_switcher::{model_switcher_overlay_title, render_model_switcher_overlay};
pub(super) use permission_modal::{
    permission_modal_actions_text, permission_modal_draft_line, permission_modal_guidance,
    permission_modal_icon, permission_modal_metadata_line, permission_modal_subject_line,
    permission_modal_summary_line, permission_modal_title, question_permission_actions_text,
    question_permission_body_text,
};
use plan_view::render_plan_view_overlay;
use prompt_stash_dialog::render_prompt_stash_list_overlay;
use session_history::{
    render_fork_selector_input, render_fork_selector_list, render_lineage_browser_overlay,
    render_session_history_overlay, render_session_rename_dialog, session_history_overlay_title,
};
use settings_editor::render_settings_editor_overlay;
use status_dialog::render_status_dialog_overlay;
#[cfg(test)]
pub(crate) use status_dialog::{
    exact_test_status_dialog_edit_attribution_counts_external_on_disk_drift,
    exact_test_status_dialog_edit_attribution_event_only_without_workspace,
    exact_test_status_dialog_edit_attribution_keeps_matching_agent_tool,
    exact_test_status_dialog_formatters_section_disabled_when_none,
    exact_test_status_dialog_formatters_section_lists_enabled_language,
    exact_test_status_dialog_mcp_rows_match_harness_states,
    exact_test_status_dialog_operator_summary_surfaces_acp_connect_bind,
    exact_test_status_dialog_operator_summary_surfaces_acp_connection,
    exact_test_status_dialog_operator_summary_surfaces_acp_session,
    exact_test_status_dialog_operator_summary_surfaces_auto_fallback_chain,
    exact_test_status_dialog_operator_summary_surfaces_binary_update_counts,
    exact_test_status_dialog_operator_summary_surfaces_binary_update_policy,
    exact_test_status_dialog_operator_summary_surfaces_binary_version,
    exact_test_status_dialog_operator_summary_surfaces_bound_settings_counts,
    exact_test_status_dialog_operator_summary_surfaces_browser_oidc_availability,
    exact_test_status_dialog_operator_summary_surfaces_browser_oidc_complete,
    exact_test_status_dialog_operator_summary_surfaces_browser_oidc_outcomes,
    exact_test_status_dialog_operator_summary_surfaces_cow_clone_last,
    exact_test_status_dialog_operator_summary_surfaces_cow_clone_outcomes,
    exact_test_status_dialog_operator_summary_surfaces_cow_fastpath,
    exact_test_status_dialog_operator_summary_surfaces_crash_recovery_action,
    exact_test_status_dialog_operator_summary_surfaces_crash_recovery_banner,
    exact_test_status_dialog_operator_summary_surfaces_crash_recovery_next,
    exact_test_status_dialog_operator_summary_surfaces_crash_scan_counts,
    exact_test_status_dialog_operator_summary_surfaces_cron_register,
    exact_test_status_dialog_operator_summary_surfaces_cron_remove,
    exact_test_status_dialog_operator_summary_surfaces_cron_schedule_counts,
    exact_test_status_dialog_operator_summary_surfaces_dashboard,
    exact_test_status_dialog_operator_summary_surfaces_demote_last,
    exact_test_status_dialog_operator_summary_surfaces_demote_last_task,
    exact_test_status_dialog_operator_summary_surfaces_demote_outcome_counts,
    exact_test_status_dialog_operator_summary_surfaces_edit_attribution,
    exact_test_status_dialog_operator_summary_surfaces_edit_attribution_first_last,
    exact_test_status_dialog_operator_summary_surfaces_fallback_and_none_demote,
    exact_test_status_dialog_operator_summary_surfaces_fallback_banner,
    exact_test_status_dialog_operator_summary_surfaces_fallback_models,
    exact_test_status_dialog_operator_summary_surfaces_fallback_outcome,
    exact_test_status_dialog_operator_summary_surfaces_foreign_discover_counts,
    exact_test_status_dialog_operator_summary_surfaces_foreign_import_last,
    exact_test_status_dialog_operator_summary_surfaces_foreign_import_next,
    exact_test_status_dialog_operator_summary_surfaces_graph_batch_first,
    exact_test_status_dialog_operator_summary_surfaces_graph_query_batch,
    exact_test_status_dialog_operator_summary_surfaces_graph_query_last,
    exact_test_status_dialog_operator_summary_surfaces_jujutsu_components,
    exact_test_status_dialog_operator_summary_surfaces_jujutsu_last_command,
    exact_test_status_dialog_operator_summary_surfaces_jujutsu_probe,
    exact_test_status_dialog_operator_summary_surfaces_landlock_support,
    exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_exchange_open,
    exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_outcomes,
    exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_remote_availability,
    exact_test_status_dialog_operator_summary_surfaces_os_sandbox_first_prepare,
    exact_test_status_dialog_operator_summary_surfaces_os_sandbox_profiles,
    exact_test_status_dialog_operator_summary_surfaces_persistent_graph,
    exact_test_status_dialog_operator_summary_surfaces_plan_view,
    exact_test_status_dialog_operator_summary_surfaces_sandbox_fs_plan,
    exact_test_status_dialog_operator_summary_surfaces_sleep_wake_availability,
    exact_test_status_dialog_operator_summary_surfaces_sleep_wake_observations,
    exact_test_status_dialog_operator_summary_surfaces_sleep_wake_policy,
    exact_test_status_dialog_operator_summary_surfaces_team_add_cancel,
    exact_test_status_dialog_operator_summary_surfaces_team_create,
    exact_test_status_dialog_operator_summary_surfaces_team_registry_counts,
    exact_test_status_dialog_operator_summary_surfaces_team_send,
    exact_test_status_dialog_operator_summary_surfaces_workspace_hub_availability,
    exact_test_status_dialog_operator_summary_surfaces_workspace_hub_bind_upload_recover,
    exact_test_status_dialog_operator_summary_surfaces_workspace_hub_outcomes,
    exact_test_status_dialog_plugins_section_surfaces_extension_descriptor,
    exact_test_status_dialog_plugins_section_surfaces_extension_discover,
    exact_test_status_dialog_plugins_section_surfaces_lifecycle_summary,
    exact_test_status_dialog_plugins_section_surfaces_plugin_activate,
    exact_test_status_dialog_plugins_section_surfaces_plugin_deactivate,
    exact_test_status_dialog_plugins_section_surfaces_plugin_install,
    exact_test_status_dialog_plugins_section_surfaces_plugin_remove,
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
            OverlayKind::CommandPalette => render_command_palette_overlay(
                frame,
                app,
                theme,
                plan.content,
                plan.composer,
                plan.palette_overlay,
            ),
            OverlayKind::TogglesMenu | OverlayKind::LineageBrowser | OverlayKind::ForkSelector => {
                render_command_palette_overlay(
                    frame,
                    app,
                    theme,
                    plan.content,
                    plan.composer,
                    plan.palette_overlay,
                )
            }
            OverlayKind::StatusDialog => render_status_dialog_overlay(frame, app, theme, plan.root),
            OverlayKind::SubagentActions => {
                render_subagent_actions_overlay(frame, app, theme, plan.root)
            }
            OverlayKind::ThemeDialog => render_theme_dialog_overlay(frame, app, theme, plan.root),
            OverlayKind::PermissionModal => {}
            OverlayKind::ErrorDetails => render_error_details_overlay(frame, app, theme, plan.root),
            OverlayKind::PromptStashList => {
                render_prompt_stash_list_overlay(frame, app, theme, plan.root)
            }
            OverlayKind::AuthDialog => render_auth_dialog_overlay(frame, app, theme, plan.root),
            OverlayKind::SettingsEditor => {
                render_settings_editor_overlay(frame, app, theme, plan.root)
            }
            OverlayKind::PlanView => render_plan_view_overlay(frame, app, theme, plan.root),
        }
    }
}

fn render_subagent_actions_overlay(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    if app.subagent_actions_session_id.is_none() {
        return;
    }

    render_overlay_dim_backdrop(frame, root);

    let width = 42.min(root.width.saturating_sub(4));
    let height = 7.min(root.height.saturating_sub(4));
    if width < 28 || height < 5 {
        return;
    }

    let overlay = Rect::new(
        root.x.saturating_add(root.width.saturating_sub(width) / 2),
        root.y
            .saturating_add(root.height.saturating_sub(height) / 2),
        width,
        height,
    );
    if !paint_command_palette_panel(frame, theme, overlay) {
        return;
    }

    let content = inset_rect(overlay, 3.min(overlay.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height < 3 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(content);
    render_command_palette_header(frame, theme, rows[0], "Subagent Actions");
    render_subagent_action_row(frame, theme, rows[2]);
}

fn render_subagent_action_row(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = Style::default()
        .fg(ui_chrome::command_palette_selection_fg(theme))
        .bg(ui_chrome::command_palette_selection_bg(theme));
    frame.render_widget(Block::default().style(row_style), area);

    let width = usize::from(area.width);
    let label = "Open";
    let description = "the subagent's session";
    let gap = width.saturating_sub(
        label
            .chars()
            .count()
            .saturating_add(description.chars().count()),
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, row_style.add_modifier(Modifier::BOLD)),
            Span::styled(" ".repeat(gap), row_style),
            Span::styled(
                truncate_plain_text(description, width.saturating_sub(label.chars().count())),
                Style::default()
                    .fg(ui_chrome::command_palette_selection_fg(theme))
                    .bg(surface),
            ),
        ])),
        area,
    );
}

fn render_session_history_side_hint(frame: &mut Frame, _theme: &Theme, root: Rect, overlay: Rect) {
    let hint = "or this directory";
    let hint_width = u16::try_from(hint.chars().count()).unwrap_or(u16::MAX);
    let x = overlay.x.saturating_add(overlay.width);
    let available = root.x.saturating_add(root.width).saturating_sub(x);
    if available == 0 || hint_width == 0 {
        return;
    }
    let width = available.min(hint_width);
    let y = overlay.y.saturating_add(overlay.height.saturating_sub(4));
    if y >= root.y.saturating_add(root.height) {
        return;
    }
    let text = truncate_plain_text(hint, usize::from(width));
    let style = Style::default().add_modifier(Modifier::BOLD);
    let buffer = frame.buffer_mut();
    for (index, ch) in text.chars().enumerate() {
        let cell_x = x.saturating_add(u16::try_from(index).unwrap_or(u16::MAX));
        if cell_x >= x.saturating_add(width) {
            break;
        }
        let cell = &mut buffer[(cell_x, y)];
        cell.set_char(ch);
        cell.set_style(style);
    }
}

fn render_command_palette_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
    composer: Option<Rect>,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    render_palette_solid_backdrop(frame, root, composer);

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
        if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
            return;
        }
        render_session_history_overlay(frame, app, theme, overlay, &title);
        render_session_history_side_hint(frame, theme, root, overlay);
        if app.session_rename_visible {
            render_session_rename_dialog(frame, app, theme, overlay);
        }
    } else if app.model_switcher_visible {
        if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
            return;
        }
        render_model_switcher_overlay(frame, app, theme, overlay, &title);
    } else if app.toggles_menu_visible {
        if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
            return;
        }
        let Some((_header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_input(frame, app, theme, input);
        render_toggles_menu_list(frame, app, theme, list);
        if app.toggles_yolo_confirmation_visible() {
            render_yolo_warning_popup(frame, theme, overlay);
        }
    } else if app.lineage_browser_visible {
        if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
            return;
        }
        let Some(inner) = command_palette_bordered_inner(overlay) else {
            return;
        };
        render_lineage_browser_overlay(frame, app, theme, inner, &title);
    } else if app.fork_selector_visible {
        if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
            return;
        }
        let Some((_header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_fork_selector_input(frame, app, theme, input);
        render_fork_selector_list(frame, app, theme, list);
    } else if !paint_command_palette_panel_titled(frame, theme, overlay, &title) {
        return;
    } else {
        let Some((_header, input, list)) = command_palette_dialog_layout(overlay) else {
            return;
        };
        render_command_palette_input(frame, app, theme, input);
        render_command_palette_list(frame, app, theme, list);
        if let Some(footer) = command_palette_footer_area(overlay) {
            render_command_palette_footer(frame, theme, footer);
        }
    }
}

fn command_palette_bordered_inner(overlay: Rect) -> Option<Rect> {
    if overlay.width <= 2 || overlay.height <= 2 {
        return None;
    }
    Some(Rect::new(
        overlay.x.saturating_add(1),
        overlay.y.saturating_add(1),
        overlay.width.saturating_sub(2),
        overlay.height.saturating_sub(2),
    ))
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
    paint_command_palette_panel_titled(frame, theme, overlay, "Commands")
}

fn paint_command_palette_panel_titled(
    frame: &mut Frame,
    theme: &Theme,
    overlay: Rect,
    title: &str,
) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    let surface = ui_chrome::command_palette_surface(theme);
    let border_style = Style::default().fg(Color::Indexed(8)).bg(surface);
    let title_style = Style::default()
        .fg(Color::Indexed(15))
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let close_style = border_style;
    frame.render_widget(Clear, overlay);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Plain)
        .border_style(border_style)
        .style(Style::default().bg(surface))
        .title(Line::from(vec![
            Span::styled("─ ", border_style),
            Span::styled(title.to_string(), title_style),
            Span::styled(" ", border_style),
        ]))
        .title(
            Line::from(vec![
                Span::styled(" [", close_style),
                Span::styled("✗", close_style),
                Span::styled("] ─", close_style),
            ])
            .right_aligned(),
        );
    frame.render_widget(block, overlay);
    true
}

fn command_palette_dialog_layout(overlay: Rect) -> Option<(Rect, Rect, Rect)> {
    if overlay.width <= 8 || overlay.height <= 6 {
        return None;
    }

    let inner = Rect::new(
        overlay.x.saturating_add(1),
        overlay.y.saturating_add(1),
        overlay.width.saturating_sub(2),
        overlay.height.saturating_sub(2),
    );
    if inner.width <= 4 || inner.height <= 4 {
        return None;
    }

    let content_x = inner.x.saturating_add(2);
    let content_width = inner.width.saturating_sub(4);
    let header = Rect::new(content_x, inner.y, content_width, 0);
    let input = Rect::new(content_x, inner.y.saturating_add(1), content_width, 1);
    let list = Rect::new(
        inner.x,
        inner.y.saturating_add(3),
        inner.width,
        inner.height.saturating_sub(4),
    );
    Some((header, input, list))
}

fn command_palette_footer_area(overlay: Rect) -> Option<Rect> {
    if overlay.width <= 4 || overlay.height <= 4 {
        return None;
    }
    let content_x = overlay.x.saturating_add(3);
    let content_width = overlay.width.saturating_sub(6);
    Some(Rect::new(
        content_x,
        overlay.y.saturating_add(overlay.height.saturating_sub(2)),
        content_width,
        1,
    ))
}

fn render_command_palette_footer(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let muted = Style::default().fg(Color::Indexed(7)).bg(surface);
    let key = Style::default()
        .fg(Color::Indexed(15))
        .bg(surface)
        .add_modifier(Modifier::BOLD);
    let spans = vec![
        Span::styled("↑/↓".to_string(), key),
        Span::styled(" nav  |  ".to_string(), muted),
        Span::styled("Enter".to_string(), key),
        Span::styled(" select  |  ".to_string(), muted),
        Span::styled("Esc".to_string(), key),
        Span::styled(" close".to_string(), muted),
    ];
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .alignment(Alignment::Center)
            .style(Style::default().bg(surface)),
        area,
    );
}

fn render_command_palette_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    let _ = (frame, theme, area, title);
}

fn render_palette_empty_message(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let empty_area = Rect::new(
        area.x.saturating_add(4),
        area.y,
        area.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            truncate_plain_text(message, usize::from(empty_area.width)),
            Style::default().bg(ui_chrome::command_palette_surface(theme)),
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

    let chrome = Style::default().fg(Color::Indexed(7)).bg(surface);
    let cursor = Style::default()
        .fg(command_palette_input_cursor(theme, app))
        .bg(surface);
    let line = if app.palette_input.is_empty() {
        let placeholder = if app.session_history_visible {
            "/ to search"
        } else if app.model_switcher_visible {
            "Filter models, providers"
        } else if app.toggles_menu_visible {
            "Filter toggles"
        } else if app.lineage_browser_visible {
            "Filter Harness session tree"
        } else {
            "search:"
        };
        if app.session_history_visible {
            let prefix = format!(" {placeholder}");
            let prefix_len = prefix.chars().count();
            let chip_len = 5usize;
            let trailing = 2usize;
            let chip_start = usize::from(area.width)
                .saturating_sub(trailing)
                .saturating_sub(chip_len);
            let gap = chip_start.saturating_sub(prefix_len);
            let plain = Style::default();
            let chip_key = Style::default().add_modifier(Modifier::BOLD);
            Line::from(vec![
                Span::styled(prefix, plain),
                Span::styled(" ".repeat(gap), plain),
                Span::styled("All ".to_string(), plain),
                Span::styled("f".to_string(), chip_key),
                Span::styled(" ".repeat(trailing), plain),
            ])
        } else {
            Line::from(vec![
                Span::styled(format!(" {placeholder}"), chrome),
                Span::styled(" ", cursor),
            ])
        }
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
            Span::styled(" search: ", chrome),
            Span::styled(before.to_string(), chrome),
            Span::styled("█", cursor),
            Span::styled(after.to_string(), chrome),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);

    let rule_y = area.y.saturating_add(1);
    let rule_area = Rect::new(
        area.x.saturating_sub(2),
        rule_y,
        area.width.saturating_add(4),
        1,
    );
    if rule_area.width > 0 {
        let rule = "─".repeat(usize::from(rule_area.width));
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                rule,
                Style::default().fg(Color::Indexed(8)).bg(surface),
            ))),
            rule_area,
        );
    }
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

    let list_area = if area.width <= 1 {
        area
    } else {
        Rect::new(
            area.x.saturating_add(1),
            area.y,
            area.width.saturating_sub(1),
            area.height,
        )
    };
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

    let mut selected_category = None;
    let mut current_category = None;
    let mut row_categories: Vec<Option<crate::keybindings::palette_model::PaletteCategory>> =
        Vec::with_capacity(rows.len());
    for row in &rows {
        match row {
            PaletteOverlayRow::Section(category) => {
                current_category = Some(*category);
                row_categories.push(current_category);
            }
            PaletteOverlayRow::Command { is_selected, .. } => {
                row_categories.push(current_category);
                if *is_selected == selected {
                    selected_category = current_category;
                }
            }
            PaletteOverlayRow::Spacer => row_categories.push(None),
        }
    }

    for (row, palette_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(list_area.x, row_y, list_area.width, 1);
        let show_thumb =
            selected_category.is_some() && row_categories.get(row) == Some(&selected_category);
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
                        show_thumb,
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
                frame.render_widget(
                    Paragraph::new(command_palette_row(
                        title,
                        description,
                        footer,
                        is_selected,
                        theme,
                        row_area.width,
                        show_thumb,
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
                .map(|e| {
                    let freeze = e.freeze_shortcut();
                    if !freeze.is_empty() {
                        freeze.to_string()
                    } else {
                        match e.dispatch {
                            PaletteDispatch::Action(action) => app.keymap.get_binding_str(action),
                            PaletteDispatch::OpenModelSwitcher => {
                                app.keymap.get_binding_str(Action::OpenModelSwitcher)
                            }
                            _ => String::new(),
                        }
                    }
                })
                .filter(|s| s != "-" && !s.is_empty())
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
    show_thumb: bool,
) -> Line<'static> {
    let _ = (description, is_selected);
    let row_width = usize::from(width);
    let gutter = 4usize;
    let body_row_width = row_width.saturating_sub(gutter);
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = Style::default().fg(Color::Indexed(15)).bg(surface);
    let label_style = Style::default().fg(Color::Indexed(15)).bg(surface);
    let prefix_style = Style::default().fg(Color::Indexed(8)).bg(surface);
    let shortcut_style = Style::default().fg(Color::Indexed(7)).bg(surface);

    let shortcut_len = if shortcut.is_empty() {
        0
    } else {
        shortcut.chars().count()
    };
    let body_width = body_row_width.saturating_sub(shortcut_len);
    let prefix = " ◆ ";
    let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
    let mut used_width = prefix.chars().count();

    let label = truncate_plain_text(label, 61usize.min(body_width.saturating_sub(used_width)));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(label, label_style));

    if used_width < body_width {
        spans.push(Span::styled(" ".repeat(body_width - used_width), row_style));
    }

    if !shortcut.is_empty() {
        spans.push(Span::styled(shortcut.to_string(), shortcut_style));
    }

    spans.push(Span::styled("   ", row_style));
    spans.push(Span::styled(
        if show_thumb { "█" } else { " " }.to_string(),
        row_style,
    ));

    Line::from(spans)
}

fn command_palette_section_row(
    label: &str,
    theme: &Theme,
    width: u16,
    show_thumb: bool,
) -> Line<'static> {
    let _ = theme;
    let row_width = usize::from(width);
    let gutter = 3usize;
    let body_row_width = row_width.saturating_sub(gutter);
    let label_style = Style::default()
        .fg(Color::Indexed(7))
        .add_modifier(Modifier::BOLD);
    let rule_style = Style::default().fg(Color::Indexed(8));
    let prefix = " ";
    let mut spans = vec![Span::raw(prefix.to_string())];
    let label = truncate_plain_text(
        label,
        body_row_width
            .saturating_sub(prefix.chars().count())
            .saturating_sub(4),
    );
    let padded_label = format!(" {label} ");
    let label_width = padded_label.chars().count();
    spans.push(Span::styled(padded_label, label_style));
    let used_width = prefix.chars().count().saturating_add(label_width);
    if used_width < body_row_width {
        let rule = "─".repeat(body_row_width.saturating_sub(used_width));
        spans.push(Span::styled(rule, rule_style));
    }
    spans.push(Span::raw("  ".to_string()));
    spans.push(Span::raw(if show_thumb { "█" } else { " " }.to_string()));
    Line::from(spans)
}

fn render_palette_solid_backdrop(frame: &mut Frame, area: Rect, preserve: Option<Rect>) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let buffer = frame.buffer_mut();
    let max_x = area.x.saturating_add(area.width);
    let max_y = area.y.saturating_add(area.height);
    let preserve = preserve.filter(|rect| rect.width > 0 && rect.height > 0);
    for y in area.y..max_y {
        for x in area.x..max_x {
            if preserve.is_some_and(|rect| {
                x >= rect.x
                    && y >= rect.y
                    && x < rect.x.saturating_add(rect.width)
                    && y < rect.y.saturating_add(rect.height)
            }) {
                continue;
            }
            let cell = &mut buffer[(x, y)];
            cell.set_symbol(" ");
            cell.set_fg(Color::Indexed(0));
            cell.set_bg(Color::Indexed(0));
        }
    }
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
