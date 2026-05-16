use std::collections::BTreeMap;
use std::path::Path;

use harness_core::proj::RunStatus;

use super::*;
use crate::text::{has_trimmed_content, trimmed_json_nested_string_field};
use crate::time_format::short_time_or_trimmed;

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
            OverlayKind::PermissionModal => {}
        }
    }
}

fn render_status_dialog_overlay(frame: &mut Frame, app: &AppState, theme: &Theme, root: Rect) {
    let Some(overlay) = status_dialog_area(root) else {
        return;
    };

    render_overlay_dim_backdrop(frame, root);
    if !paint_command_palette_panel(frame, theme, overlay) {
        return;
    }

    let content = inset_rect(overlay, 2.min(overlay.width.saturating_sub(1)), 1);
    if content.width == 0 || content.height == 0 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(content);
    render_status_dialog_header(frame, theme, chunks[0]);
    render_status_dialog_body(frame, theme, chunks[1], status_dialog_body(app, theme));
}

fn render_status_dialog_body(frame: &mut Frame, theme: &Theme, area: Rect, body: Text<'static>) {
    frame.render_widget(
        Paragraph::new(body)
            .style(Style::default().bg(ui_chrome::command_palette_surface(theme)))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn status_dialog_area(root: Rect) -> Option<Rect> {
    let width = 64.min(root.width.saturating_sub(4));
    let height = 18.min(root.height.saturating_sub(4));
    if width < 32 || height < 8 {
        return None;
    }
    Some(Rect::new(
        root.x.saturating_add(root.width.saturating_sub(width) / 2),
        root.y
            .saturating_add(root.height.saturating_sub(height) / 2),
        width,
        height,
    ))
}

fn render_status_dialog_header(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let title = "Status";
    let esc = "esc";
    let title_width = title.chars().count();
    let esc_width = esc.chars().count();
    let gap = usize::from(area.width).saturating_sub(title_width.saturating_add(esc_width));
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ".repeat(gap), Style::default().bg(surface)),
            Span::styled(esc, Style::default().fg(theme.text.secondary).bg(surface)),
        ])),
        area,
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusDialogTone {
    Success,
    Error,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusDialogRow {
    name: String,
    suffix: Option<String>,
    tone: StatusDialogTone,
    enabled: bool,
}

fn status_dialog_body(app: &AppState, theme: &Theme) -> Text<'static> {
    status_dialog_body_from_rows(
        status_dialog_workflow_rows(app),
        status_dialog_mcp_rows(),
        status_dialog_lsp_rows(app),
        status_dialog_continuation_rows(app),
        theme,
    )
}

fn status_dialog_body_from_rows(
    workflow_rows: Vec<StatusDialogRow>,
    mcp_rows: Vec<StatusDialogRow>,
    lsp_rows: Vec<StatusDialogRow>,
    continuation_rows: Vec<StatusDialogRow>,
    theme: &Theme,
) -> Text<'static> {
    let mut lines = Vec::new();
    append_status_dialog_workflow_section(&mut lines, workflow_rows, theme);
    append_status_dialog_mcp_section(&mut lines, mcp_rows, theme);
    append_status_dialog_lsp_section(&mut lines, lsp_rows, theme);
    append_status_dialog_continuation_section(&mut lines, continuation_rows, theme);
    append_status_dialog_formatters_section(&mut lines, theme);
    append_status_dialog_plugins_section(&mut lines, theme);
    Text::from(lines)
}

fn append_status_dialog_workflow_section(
    lines: &mut Vec<Line<'static>>,
    rows: Vec<StatusDialogRow>,
    theme: &Theme,
) {
    if rows.is_empty() {
        lines.push(status_dialog_plain_line("Workflow: inactive", theme));
        lines.push(Line::default());
        return;
    }

    lines.push(status_dialog_plain_line("Workflow", theme));
    append_status_dialog_rows(lines, rows, theme);
    lines.push(Line::default());
}

fn append_status_dialog_mcp_section(
    lines: &mut Vec<Line<'static>>,
    rows: Vec<StatusDialogRow>,
    theme: &Theme,
) {
    if rows.is_empty() {
        lines.push(status_dialog_plain_line("No MCP Servers", theme));
        lines.push(Line::default());
        return;
    }

    lines.push(status_dialog_plain_line(
        format!("{} MCP Servers", status_dialog_enabled_mcp_count(&rows)),
        theme,
    ));
    append_status_dialog_rows(lines, rows, theme);
    lines.push(Line::default());
}

fn status_dialog_enabled_mcp_count(rows: &[StatusDialogRow]) -> usize {
    rows.iter().filter(|row| row.enabled).count()
}

fn append_status_dialog_lsp_section(
    lines: &mut Vec<Line<'static>>,
    rows: Vec<StatusDialogRow>,
    theme: &Theme,
) {
    if rows.is_empty() {
        return;
    }

    lines.push(status_dialog_plain_line(
        format!("{} LSP Servers", rows.len()),
        theme,
    ));
    append_status_dialog_rows(lines, rows, theme);
    lines.push(Line::default());
}

fn append_status_dialog_continuation_section(
    lines: &mut Vec<Line<'static>>,
    rows: Vec<StatusDialogRow>,
    theme: &Theme,
) {
    if rows.is_empty() {
        lines.push(status_dialog_plain_line("Continuation: inactive", theme));
        lines.push(Line::default());
        return;
    }

    lines.push(status_dialog_plain_line("Continuation", theme));
    append_status_dialog_rows(lines, rows, theme);
    lines.push(Line::default());
}

fn append_status_dialog_formatters_section(lines: &mut Vec<Line<'static>>, theme: &Theme) {
    lines.push(status_dialog_plain_line("No Formatters", theme));
    lines.push(Line::default());
}

fn append_status_dialog_plugins_section(lines: &mut Vec<Line<'static>>, theme: &Theme) {
    lines.push(status_dialog_plain_line("No Plugins", theme));
}

fn append_status_dialog_rows(
    lines: &mut Vec<Line<'static>>,
    rows: Vec<StatusDialogRow>,
    theme: &Theme,
) {
    for row in rows {
        lines.push(status_dialog_row_line(row, theme));
    }
}

fn status_dialog_plain_line(text: impl Into<String>, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        text.into(),
        Style::default()
            .fg(theme.text.primary)
            .bg(ui_chrome::command_palette_surface(theme)),
    ))
}

fn status_dialog_row_line(row: StatusDialogRow, theme: &Theme) -> Line<'static> {
    let surface = ui_chrome::command_palette_surface(theme);
    let dot_color = match row.tone {
        StatusDialogTone::Success => theme.status.success,
        StatusDialogTone::Error => theme.status.error,
        StatusDialogTone::Muted => theme.text.secondary,
    };
    let mut spans = vec![
        Span::styled("• ", Style::default().fg(dot_color).bg(surface)),
        Span::styled(
            row.name,
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    if let Some(suffix) = row.suffix {
        spans.push(Span::styled(
            format!(" {suffix}"),
            Style::default().fg(theme.text.secondary).bg(surface),
        ));
    }
    Line::from(spans)
}

fn status_dialog_workflow_rows(app: &AppState) -> Vec<StatusDialogRow> {
    app.workflow_status_rows()
        .into_iter()
        .map(|row| {
            let name = row
                .title
                .as_deref()
                .filter(|title| !title.trim().is_empty())
                .unwrap_or(&row.workflow_id)
                .to_string();
            let mut suffix = format!("{} · {} · owner {}", row.mode, row.status, row.owner);
            if row.evidence_count > 0 {
                suffix.push_str(&format!(" · {} evidence", row.evidence_count));
            }
            if row.operator_decision_count > 0 {
                suffix.push_str(&format!(" · {} decisions", row.operator_decision_count));
            }
            if let Some(category) = row.latest_evidence_category.as_deref() {
                suffix.push_str(&format!(" · latest {category}"));
            }
            let tone = if row.is_blocked() {
                StatusDialogTone::Error
            } else if row.terminal {
                StatusDialogTone::Muted
            } else {
                StatusDialogTone::Success
            };
            StatusDialogRow {
                name: sanitize_status_dialog_text(&name),
                suffix: Some(sanitize_status_dialog_text(&suffix)),
                tone,
                enabled: !row.terminal,
            }
        })
        .collect()
}

fn status_dialog_mcp_rows() -> Vec<StatusDialogRow> {
    let Some(integrations) = harness_core::config::registered_integrations_config() else {
        return Vec::new();
    };

    let connection_states = integrations
        .mcp
        .servers
        .keys()
        .filter_map(|name| {
            harness_core::config::registered_mcp_server_connection_state(name)
                .map(|state| (name.clone(), state))
        })
        .collect::<BTreeMap<_, _>>();

    status_dialog_mcp_rows_from_config(integrations, &connection_states)
}

fn status_dialog_mcp_rows_from_config(
    integrations: harness_core::config::IntegrationsConfig,
    connection_states: &BTreeMap<String, harness_core::config::McpServerConnectionState>,
) -> Vec<StatusDialogRow> {
    integrations
        .mcp
        .servers
        .into_iter()
        .map(|(name, server)| {
            if !server.enabled() {
                return StatusDialogRow {
                    name: sanitize_status_dialog_text(&name),
                    suffix: Some("Disabled in configuration".to_string()),
                    tone: StatusDialogTone::Muted,
                    enabled: false,
                };
            }

            match connection_states.get(&name) {
                Some(harness_core::config::McpServerConnectionState::Connected) => {
                    StatusDialogRow {
                        name: sanitize_status_dialog_text(&name),
                        suffix: Some("Connected".to_string()),
                        tone: StatusDialogTone::Success,
                        enabled: true,
                    }
                }
                Some(harness_core::config::McpServerConnectionState::Failed(error)) => {
                    StatusDialogRow {
                        name: sanitize_status_dialog_text(&name),
                        suffix: Some(sanitize_status_dialog_text(error)),
                        tone: StatusDialogTone::Error,
                        enabled: true,
                    }
                }
                None => StatusDialogRow {
                    name: sanitize_status_dialog_text(&name),
                    suffix: Some("Checking".to_string()),
                    tone: StatusDialogTone::Muted,
                    enabled: true,
                },
            }
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn exact_test_status_dialog_mcp_rows_match_harness_states() {
    let mut integrations = harness_core::config::IntegrationsConfig::default();
    integrations.mcp.servers.insert(
        "connected".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://example.com/mcp".to_string(),
            headers: BTreeMap::new(),
            timeout_secs: 10,
            enabled: true,
        },
    );
    integrations.mcp.servers.insert(
        "disabled".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://disabled.example.com/mcp".to_string(),
            headers: BTreeMap::new(),
            timeout_secs: 10,
            enabled: false,
        },
    );
    integrations.mcp.servers.insert(
        "failed".to_string(),
        harness_core::config::McpServerConfig::Http {
            endpoint: "https://failed.example.com/mcp".to_string(),
            headers: BTreeMap::new(),
            timeout_secs: 10,
            enabled: true,
        },
    );

    let rows = status_dialog_mcp_rows_from_config(
        integrations,
        &BTreeMap::from([
            (
                "connected".to_string(),
                harness_core::config::McpServerConnectionState::Connected,
            ),
            (
                "failed".to_string(),
                harness_core::config::McpServerConnectionState::Failed(
                    "connection refused".to_string(),
                ),
            ),
        ]),
    );

    assert_eq!(rows.len(), 3);
    assert_eq!(status_dialog_enabled_mcp_count(&rows), 2);
    assert_eq!(rows[0].name, "connected");
    assert_eq!(rows[0].suffix.as_deref(), Some("Connected"));
    assert_eq!(rows[0].tone, StatusDialogTone::Success);
    assert_eq!(rows[1].name, "disabled");
    assert_eq!(rows[1].suffix.as_deref(), Some("Disabled in configuration"));
    assert_eq!(rows[1].tone, StatusDialogTone::Muted);
    assert_eq!(rows[2].name, "failed");
    assert_eq!(rows[2].suffix.as_deref(), Some("connection refused"));
    assert_eq!(rows[2].tone, StatusDialogTone::Error);
}

#[cfg(test)]
pub(crate) fn exact_test_status_dialog_continuation_rows_show_active_loop() {
    let mut app = AppState::new_live(Some(std::path::PathBuf::from("/tmp/session")), false, None);
    app.active_continuation = Some(crate::app::ContinuationDisplayState {
        continuation_id: "cont_000001".to_string(),
        mode: "ultrawork".to_string(),
        command: "/ulw-loop".to_string(),
        iteration: 3,
        status: "reminder queued".to_string(),
    });

    let rows = status_dialog_continuation_rows(&app);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "ultrawork via /ulw-loop");
    assert_eq!(
        rows[0].suffix.as_deref(),
        Some("reminder queued · iteration 3")
    );
    assert_eq!(rows[0].tone, StatusDialogTone::Success);
    assert!(rows[0].enabled);
}

#[cfg(test)]
pub(crate) fn exact_test_status_dialog_render_snapshot_covers_harness_sections() {
    let theme = Theme::default();
    let mcp_rows = vec![
        StatusDialogRow {
            name: "connected".to_string(),
            suffix: Some("Connected".to_string()),
            tone: StatusDialogTone::Success,
            enabled: true,
        },
        StatusDialogRow {
            name: "disabled".to_string(),
            suffix: Some("Disabled in configuration".to_string()),
            tone: StatusDialogTone::Muted,
            enabled: false,
        },
        StatusDialogRow {
            name: "failed".to_string(),
            suffix: Some("connection refused".to_string()),
            tone: StatusDialogTone::Error,
            enabled: true,
        },
    ];
    let lsp_rows = vec![StatusDialogRow {
        name: "rust".to_string(),
        suffix: Some("/workspace".to_string()),
        tone: StatusDialogTone::Success,
        enabled: true,
    }];

    let backend = ratatui::backend::TestBackend::new(80, 24);
    let mut terminal = ratatui::Terminal::new(backend).expect("create terminal");
    terminal
        .draw(|frame| {
            let root = Rect::new(0, 0, 80, 24);
            let overlay = status_dialog_area(root).expect("status dialog area");
            render_overlay_dim_backdrop(frame, root);
            assert!(paint_command_palette_panel(frame, &theme, overlay));
            let content = inset_rect(overlay, 2.min(overlay.width.saturating_sub(1)), 1);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(0)])
                .split(content);
            render_status_dialog_header(frame, &theme, chunks[0]);
            render_status_dialog_body(
                frame,
                &theme,
                chunks[1],
                status_dialog_body_from_rows(Vec::new(), mcp_rows, lsp_rows, Vec::new(), &theme),
            );
        })
        .expect("draw status dialog");

    insta::assert_snapshot!(
        "status_dialog_render_snapshot_covers_harness_sections",
        format!("{:#?}", terminal.backend().buffer())
    );
}

fn status_dialog_lsp_rows(app: &AppState) -> Vec<StatusDialogRow> {
    if harness_core::config::registered_lsp_config().disabled {
        return Vec::new();
    }

    let mut rows = BTreeMap::new();
    for activity in &app.activities {
        for tool_call in &activity.tool_calls {
            if !matches!(
                tool_call.effective_tool_id(),
                "lsp" | "lsp.rename" | "code.lsp" | "code.lsp.rename"
            ) || matches!(
                tool_call.status,
                ToolCallDisplayStatus::PendingPermission | ToolCallDisplayStatus::Queued
            ) {
                continue;
            }
            let Some(id) = status_dialog_lsp_server_name(tool_call) else {
                continue;
            };
            let root = status_dialog_lsp_root(tool_call, app.session_path.as_deref())
                .unwrap_or_else(|| "unknown".to_string());
            let tone = if tool_call.status == ToolCallDisplayStatus::Failed {
                StatusDialogTone::Error
            } else {
                StatusDialogTone::Success
            };
            rows.insert(
                id.clone(),
                StatusDialogRow {
                    name: id,
                    suffix: Some(root),
                    tone,
                    enabled: true,
                },
            );
        }
    }
    rows.into_values().collect()
}

fn status_dialog_continuation_rows(app: &AppState) -> Vec<StatusDialogRow> {
    let Some(active) = app.active_continuation.as_ref() else {
        return Vec::new();
    };
    vec![StatusDialogRow {
        name: format!("{} via {}", active.mode, active.command),
        suffix: Some(format!(
            "{} · iteration {}",
            active.status, active.iteration
        )),
        tone: StatusDialogTone::Success,
        enabled: true,
    }]
}

fn status_dialog_lsp_server_name(tool_call: &crate::app::ToolCallEntry) -> Option<String> {
    trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["server", "id"])
        .or_else(|| {
            trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["server", "name"])
        })
        .or_else(|| ui_lsp::server_name_from_args(&tool_call.args_summary))
        .map(|name| sanitize_status_dialog_text(&name))
        .filter(|name| !name.is_empty())
}

fn status_dialog_lsp_root(
    tool_call: &crate::app::ToolCallEntry,
    session_path: Option<&Path>,
) -> Option<String> {
    trimmed_json_nested_string_field(tool_call.output_json.as_ref(), &["server", "root"])
        .or_else(|| ui_lsp::path_root_from_args(&tool_call.args_summary))
        .or_else(|| session_path.and_then(Path::to_str).map(str::to_string))
        .map(|root| sanitize_status_dialog_text(&root))
        .filter(|root| !root.is_empty())
}

fn sanitize_status_dialog_text(value: &str) -> String {
    value
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
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
                app.slash_command_description(command),
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

fn render_session_history_overlay(
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
    let scope = Rect::new(content_x, overlay.y.saturating_add(4), content_width, 1);
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
    render_session_history_scope(frame, app, theme, scope);
    render_session_history_list(frame, app, theme, list);
    render_session_history_actions(frame, theme, actions);
}

fn render_session_history_scope(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 {
        return;
    }

    let scope = match app.startup_launcher_action {
        crate::app::StartupLauncherAction::ContinueSession => "Interactive histories",
        crate::app::StartupLauncherAction::ReplaySession => {
            "Read-only replays · interactive and prompt runs stay available"
        }
        crate::app::StartupLauncherAction::NewSession => "Saved sessions",
    };
    frame.render_widget(
        Paragraph::new(truncate_plain_text(scope, usize::from(area.width)))
            .style(
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(ui_chrome::command_palette_surface(theme)),
            )
            .alignment(Alignment::Left),
        area,
    );
}

fn render_lineage_browser_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    title: &str,
) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
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

fn render_fork_selector_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
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
    if vm.empty_message.is_some() {
        render_fork_selector_empty_message(frame, theme, area);
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

fn render_fork_selector_empty_message(frame: &mut Frame, theme: &Theme, area: Rect) {
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
            truncate_plain_text("No results found", usize::from(empty_area.width)),
            Style::default()
                .fg(ui_chrome::command_palette_muted(theme))
                .bg(surface),
        ))),
        empty_area,
    );
}

fn render_palette_empty_message(frame: &mut Frame, theme: &Theme, area: Rect, message: &str) {
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

fn render_fork_selector_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
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

fn render_model_switcher_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Rect,
    title: &str,
) {
    if overlay.width <= 8 || overlay.height <= 5 {
        return;
    }

    let header = Rect::new(
        overlay.x.saturating_add(4),
        overlay.y.saturating_add(1),
        overlay.width.saturating_sub(8),
        1,
    );
    let input = Rect::new(
        overlay.x.saturating_add(4),
        overlay.y.saturating_add(3),
        overlay.width.saturating_sub(8),
        1,
    );
    let list = Rect::new(
        overlay.x.saturating_add(1),
        overlay.y.saturating_add(5),
        overlay.width.saturating_sub(2),
        overlay.height.saturating_sub(6),
    );

    render_model_select_header(frame, theme, header, title);
    render_model_select_input(frame, app, theme, input);
    render_model_switcher_list(frame, app, theme, list);
}

fn render_model_switcher_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = model_select_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let rows = model_switcher_rows(app);
    if rows.is_empty() {
        let empty_area = Rect::new(
            area.x.saturating_add(3),
            area.y,
            area.width.saturating_sub(3),
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No results found",
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(area.height).max(1);
    let selected = app
        .model_selected
        .min(app.model_filtered.len().saturating_sub(1));
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, ModelSwitcherRow::Option { filtered_index, .. } if *filtered_index == selected))
        .unwrap_or(0);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

    for (row_index, row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row_index - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        match row {
            ModelSwitcherRow::Spacer => {
                frame.render_widget(
                    Block::default().style(Style::default().bg(surface)),
                    row_area,
                );
            }
            ModelSwitcherRow::Category(category) => {
                frame.render_widget(
                    Paragraph::new(model_switcher_category_row(category, theme, row_area.width)),
                    row_area,
                );
            }
            ModelSwitcherRow::Option {
                filtered_index,
                option_index,
            } => {
                let Some(option) = app.model_options.get(*option_index) else {
                    continue;
                };
                let is_selected = *filtered_index == selected;
                frame.render_widget(
                    Block::default().style(model_switcher_option_row_style(theme, is_selected)),
                    row_area,
                );
                frame.render_widget(
                    Paragraph::new(model_switcher_row(
                        option,
                        app,
                        is_selected,
                        has_trimmed_content(&app.palette_input),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
        }
    }
}

fn model_switcher_overlay_title(app: &AppState) -> String {
    let _ = app;
    "Select model".to_string()
}

fn model_switcher_row(
    option: &crate::app::ModelOption,
    app: &AppState,
    is_selected: bool,
    flatten: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_style = model_switcher_option_row_style(theme, is_selected);
    let selected_fg = model_select_selected_fg(theme);
    let title_style = if is_selected {
        row_style.fg(selected_fg).add_modifier(Modifier::BOLD)
    } else if app.is_current_model_option(option) {
        row_style.fg(model_select_primary(theme))
    } else {
        row_style.fg(model_select_text(theme))
    };
    let meta_style = if is_selected {
        row_style.fg(selected_fg)
    } else {
        row_style.fg(model_select_muted(theme))
    };

    let row_width = usize::from(width);
    let mut spans = Vec::new();
    let is_current = app.is_current_model_option(option);
    let leading_padding = if is_current { 1 } else { 3 }.min(row_width);
    if leading_padding > 0 {
        spans.push(Span::styled(" ".repeat(leading_padding), row_style));
    }
    let mut used_width = leading_padding;

    if is_current && used_width < row_width {
        let marker_style = if is_selected {
            row_style.fg(selected_fg)
        } else {
            row_style.fg(model_select_primary(theme))
        };
        spans.push(Span::styled("●", marker_style));
        used_width = used_width.saturating_add(1);
    }

    let title_padding = 3.min(row_width.saturating_sub(used_width));
    if title_padding > 0 {
        spans.push(Span::styled(" ".repeat(title_padding), row_style));
        used_width = used_width.saturating_add(title_padding);
    }

    let footer = flatten.then(|| option.selector_category());
    let footer_width = footer.map(str::chars).map(Iterator::count).unwrap_or(0);
    let title_budget = row_width
        .saturating_sub(used_width)
        .saturating_sub(footer_width)
        .saturating_sub(usize::from(footer_width > 0))
        .min(61);
    let title = truncate_plain_text(option.selector_title(), title_budget);
    used_width = used_width.saturating_add(title.chars().count());
    spans.push(Span::styled(title, title_style));

    if let Some(footer) = footer {
        let gap = row_width
            .saturating_sub(used_width)
            .saturating_sub(footer_width);
        if gap > 0 {
            spans.push(Span::styled(" ".repeat(gap), row_style));
            used_width = used_width.saturating_add(gap);
        }
        if used_width < row_width {
            spans.push(Span::styled(
                truncate_plain_text(footer, row_width.saturating_sub(used_width)),
                meta_style,
            ));
            used_width = row_width;
        }
    }

    if used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

enum ModelSwitcherRow {
    Spacer,
    Category(String),
    Option {
        filtered_index: usize,
        option_index: usize,
    },
}

fn model_switcher_rows(app: &AppState) -> Vec<ModelSwitcherRow> {
    if !has_trimmed_content(&app.palette_input) {
        let mut rows = Vec::new();
        let mut previous_category: Option<String> = None;
        for (filtered_index, option_index) in app.model_filtered.iter().copied().enumerate() {
            let Some(option) = app.model_options.get(option_index) else {
                continue;
            };
            let category = option.selector_category().to_string();
            if previous_category.as_deref() != Some(category.as_str()) {
                if previous_category.is_some() {
                    rows.push(ModelSwitcherRow::Spacer);
                }
                rows.push(ModelSwitcherRow::Category(category.clone()));
                previous_category = Some(category);
            }
            rows.push(ModelSwitcherRow::Option {
                filtered_index,
                option_index,
            });
        }
        return rows;
    }

    app.model_filtered
        .iter()
        .copied()
        .enumerate()
        .map(|(filtered_index, option_index)| ModelSwitcherRow::Option {
            filtered_index,
            option_index,
        })
        .collect()
}

fn paint_model_select_panel(frame: &mut Frame, theme: &Theme, overlay: Rect) -> bool {
    if overlay.width == 0 || overlay.height == 0 {
        return false;
    }

    frame.render_widget(Clear, overlay);
    frame.render_widget(
        Block::default().style(Style::default().bg(model_select_surface(theme))),
        overlay,
    );
    true
}

fn render_model_select_header(frame: &mut Frame, theme: &Theme, area: Rect, title: &str) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = model_select_surface(theme);
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
                .fg(model_select_text(theme))
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(esc)
            .alignment(Alignment::Right)
            .style(Style::default().fg(model_select_muted(theme)).bg(surface)),
        columns[1],
    );
}

fn render_model_select_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = model_select_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let line = if app.palette_input.is_empty() {
        Line::from(vec![
            Span::styled(
                "█",
                Style::default().fg(model_select_primary(theme)).bg(surface),
            ),
            Span::styled(
                " Search",
                Style::default().fg(model_select_muted(theme)).bg(surface),
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
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ),
            Span::styled(
                "█",
                Style::default().fg(model_select_primary(theme)).bg(surface),
            ),
            Span::styled(
                after.to_string(),
                Style::default().fg(model_select_muted(theme)).bg(surface),
            ),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn model_switcher_category_row(category: &str, theme: &Theme, width: u16) -> Line<'static> {
    let surface = model_select_surface(theme);
    let row_width = usize::from(width);
    let padding = 3.min(row_width);
    let mut used_width = padding;
    let mut spans = Vec::new();
    if padding > 0 {
        spans.push(Span::styled(
            " ".repeat(padding),
            Style::default().bg(surface),
        ));
    }
    let label = truncate_plain_text(category, row_width.saturating_sub(used_width));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(
        label,
        Style::default()
            .fg(model_select_primary(theme))
            .bg(surface)
            .add_modifier(Modifier::BOLD),
    ));
    if used_width < row_width {
        spans.push(Span::styled(
            " ".repeat(row_width - used_width),
            Style::default().bg(surface),
        ));
    }
    Line::from(spans)
}

fn model_switcher_option_row_style(theme: &Theme, is_selected: bool) -> Style {
    if is_selected {
        Style::default()
            .fg(model_select_selected_fg(theme))
            .bg(model_select_primary(theme))
    } else {
        Style::default().bg(model_select_surface(theme))
    }
}

const fn model_select_surface(theme: &Theme) -> Color {
    theme.surface.panel_elevated
}

const fn model_select_primary(theme: &Theme) -> Color {
    theme.status.info
}

const fn model_select_text(theme: &Theme) -> Color {
    theme.text.primary
}

const fn model_select_muted(theme: &Theme) -> Color {
    theme.text.secondary
}

const fn model_select_selected_fg(theme: &Theme) -> Color {
    theme.text.inverse
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
        let empty_area = Rect::new(
            list_area.x.saturating_add(3),
            list_area.y,
            list_area.width.saturating_sub(3),
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No results found",
                Style::default()
                    .fg(ui_chrome::command_palette_muted(theme))
                    .bg(ui_chrome::command_palette_surface(theme)),
            ))),
            empty_area,
        );
        return;
    }

    let visible_rows = usize::from(list_area.height);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let rows = palette_overlay_rows(app);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, PaletteOverlayRow::Command { command, .. } if *command == app.palette_filtered[selected]))
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
            PaletteOverlayRow::Section(section) => {
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(
                        section.label(),
                        theme,
                        row_area.width,
                    )),
                    row_area,
                );
            }
            PaletteOverlayRow::Command {
                command,
                selected_index,
            } => {
                let is_selected = *selected_index == selected;
                if is_selected {
                    frame.render_widget(
                        Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                        row_area,
                    );
                }

                frame.render_widget(
                    Paragraph::new(command_palette_row(
                        Action::palette_command_label(command),
                        palette_command_description(command),
                        Action::palette_command_shortcut(command),
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

fn render_toggles_menu_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
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

    let rows = toggles_overlay_rows(app);
    if rows.is_empty() {
        render_palette_empty_message(frame, theme, list_area, "No toggles found");
        return;
    }

    let visible_rows = usize::from(list_area.height);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, TogglesOverlayRow::Toggle(toggle) if toggle.selected))
        .unwrap_or(0);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

    for (row, toggle_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = list_area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(list_area.x, row_y, list_area.width, 1);
        match toggle_row {
            TogglesOverlayRow::Spacer => {
                frame.render_widget(
                    Block::default()
                        .style(Style::default().bg(ui_chrome::command_palette_surface(theme))),
                    row_area,
                );
            }
            TogglesOverlayRow::Section(section) => {
                frame.render_widget(
                    Paragraph::new(command_palette_section_row(section, theme, row_area.width)),
                    row_area,
                );
            }
            TogglesOverlayRow::Toggle(toggle) => {
                if toggle.selected {
                    frame.render_widget(
                        Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                        row_area,
                    );
                }
                frame.render_widget(
                    Paragraph::new(toggle_menu_row(toggle, theme, row_area.width)),
                    row_area,
                );
            }
        }
    }
}

enum TogglesOverlayRow {
    Spacer,
    Section(&'static str),
    Toggle(crate::app::ToggleMenuRow),
}

fn toggles_overlay_rows(app: &AppState) -> Vec<TogglesOverlayRow> {
    let mut rows = Vec::new();
    let mut last_section = None;
    for toggle in app.toggle_menu_rows() {
        if Some(toggle.section) != last_section {
            if last_section.is_some() {
                rows.push(TogglesOverlayRow::Spacer);
            }
            rows.push(TogglesOverlayRow::Section(toggle.section));
            last_section = Some(toggle.section);
        }
        rows.push(TogglesOverlayRow::Toggle(toggle));
    }
    rows
}

fn toggle_menu_row(toggle: &crate::app::ToggleMenuRow, theme: &Theme, width: u16) -> Line<'static> {
    let surface = ui_chrome::command_palette_surface(theme);
    let row_style = if toggle.selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default().bg(surface)
    };
    let label_style = if toggle.selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(ui_chrome::command_palette_title(theme))
    };
    let description_style = if toggle.selected {
        row_style
    } else {
        row_style.fg(ui_chrome::command_palette_muted(theme))
    };
    let state = if toggle.enabled { "●" } else { "○" };
    let state_label = if toggle.enabled { "on" } else { "off" };
    let label = format!(" {state} {}", sanitize_toggle_text(&toggle.label));
    let meta = format!(
        "{} · {state_label}",
        sanitize_toggle_text(&toggle.description)
    );
    split_title_meta_row(
        label,
        meta,
        label_style,
        description_style,
        row_style,
        width,
    )
}

fn sanitize_toggle_text(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_control())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn render_yolo_warning_popup(frame: &mut Frame, theme: &Theme, overlay: Rect) {
    let width = 54.min(overlay.width.saturating_sub(4));
    let height = 7.min(overlay.height.saturating_sub(2));
    if width < 32 || height < 5 {
        return;
    }
    let area = Rect::new(
        overlay.x + overlay.width.saturating_sub(width) / 2,
        overlay.y + overlay.height.saturating_sub(height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, area);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Confirm YOLO mode ")
        .style(
            Style::default()
                .fg(theme.status.warning)
                .bg(ui_chrome::command_palette_surface(theme)),
        );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let text = Text::from(vec![
        Line::from("YOLO marks every menu entry on."),
        Line::from("Coordinator permissions still apply."),
        Line::from(""),
        Line::from("Enter confirm   Esc cancel"),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(ui_chrome::command_palette_surface(theme)))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

enum PaletteOverlayRow<'a> {
    Spacer,
    Section(crate::keybindings::PaletteCommandSection),
    Command {
        command: &'a str,
        selected_index: usize,
    },
}

fn palette_overlay_rows(app: &AppState) -> Vec<PaletteOverlayRow<'_>> {
    let mut rows = Vec::new();
    let mut last_section = None;

    for (selected_index, command) in app.palette_filtered.iter().enumerate() {
        let section = Action::palette_command_section(command.as_str());
        if section != last_section {
            if let Some(section) = section {
                if last_section.is_some() {
                    rows.push(PaletteOverlayRow::Spacer);
                }
                rows.push(PaletteOverlayRow::Section(section));
            }
            last_section = section;
        }
        rows.push(PaletteOverlayRow::Command {
            command,
            selected_index,
        });
    }

    rows
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
        shortcut.chars().count().saturating_add(2)
    };
    let body_width = content_width.saturating_sub(reserved_shortcut);
    let prefix = "      ";
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
        spans.push(Span::styled("  ", row_style));
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

fn session_history_overlay_title(app: &AppState) -> String {
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
        let category = session_history_category_label(entry);
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
    let row_style = session_history_row_style(theme, is_selected);
    let text_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else if current {
        Style::default().fg(ui_chrome::fork_selector_cursor())
    } else {
        Style::default().fg(theme.text.primary)
    };
    let footer_style = if is_selected {
        row_style
    } else {
        Style::default().fg(theme.text.secondary)
    };
    let marker_style = if is_selected {
        row_style
    } else {
        Style::default().fg(ui_chrome::fork_selector_cursor())
    };
    let left_padding = if current { 1usize } else { 3usize };
    let marker = if current { "●" } else { "" };
    let marker_gap = usize::from(current);
    let footer = match app.startup_launcher_action {
        crate::app::StartupLauncherAction::ContinueSession if !entry.catalog.is_resumable => entry
            .catalog
            .resume_disabled_reason
            .clone()
            .unwrap_or_else(|| "continue unavailable".to_string()),
        crate::app::StartupLauncherAction::ContinueSession => "continue ready".to_string(),
        crate::app::StartupLauncherAction::ReplaySession => "replay ready".to_string(),
        crate::app::StartupLauncherAction::NewSession => session_history_footer_label(entry),
    };
    let footer_width = footer.chars().count();
    let title_padding = 3usize;
    let fixed_width = left_padding
        .saturating_add(marker.chars().count())
        .saturating_add(marker_gap)
        .saturating_add(title_padding)
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
            Span::styled("delete", action_style),
            Span::styled(" ctrl+d  ", key_style),
            Span::styled("rename", action_style),
            Span::styled(" ctrl+r", key_style),
        ]))
        .style(Style::default().bg(surface)),
        area,
    );
}

fn palette_command_description(command: &str) -> &'static str {
    Action::palette_commands()
        .iter()
        .find_map(|(candidate, description)| (*candidate == command).then_some(*description))
        .unwrap_or("")
}

pub(super) fn permission_modal_metadata_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    let subject = permission
        .tool_label
        .as_deref()
        .map(|tool| format!("tool {tool}"))
        .or_else(|| {
            permission
                .tool_call_id
                .as_deref()
                .map(|tool_call_id| format!("call {tool_call_id}"))
        })
        .unwrap_or_else(|| format!("perm {}", permission.permission_id));

    format!(
        "{} · dig {} · timeout {}s",
        subject,
        abbreviated_digest(&permission.request_digest),
        permission.timeout_ms / 1_000,
    )
}

fn abbreviated_digest(digest: &str) -> String {
    let mut short = digest.chars().take(6).collect::<String>();
    if digest.chars().count() > 6 {
        short.push('…');
    }
    short
}

pub(super) fn permission_modal_icon(permission: &crate::app::ActivePermissionView) -> &'static str {
    let kind = permission.kind.as_str();
    if kind.eq_ignore_ascii_case("question")
        || kind.eq_ignore_ascii_case("ask")
        || kind.eq_ignore_ascii_case("ask_user")
    {
        return "?";
    }
    if kind.eq_ignore_ascii_case("edit")
        || kind.eq_ignore_ascii_case("edit_fs")
        || kind.eq_ignore_ascii_case("lsp")
    {
        return "→";
    }
    if kind.eq_ignore_ascii_case("shell") || kind.eq_ignore_ascii_case("bash") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("task") {
        return "#";
    }
    if kind.eq_ignore_ascii_case("webfetch") {
        return "%";
    }
    if kind.eq_ignore_ascii_case("websearch") {
        return "◈";
    }
    if kind.eq_ignore_ascii_case("codesearch") {
        return "◇";
    }
    "⚙"
}

pub(super) fn permission_modal_subject_line(
    permission: &crate::app::ActivePermissionView,
) -> String {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        return "Answer operator question".to_string();
    }

    let summary = permission.summary.trim();
    if !summary.is_empty() && !summary.starts_with('{') && !summary.starts_with('[') {
        return summary.to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Review {tool}"))
        .unwrap_or_else(|| format!("Review {}", permission.kind.replace('_', " ")))
}

pub(super) fn permission_modal_title(
    permission: &crate::app::ActivePermissionView,
) -> &'static str {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Question required"
    } else {
        "Permission required"
    }
}

pub(super) fn permission_modal_guidance(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> &'static str {
    if submission_pending {
        "Decision recorded. Wait for confirmation before sending another turn."
    } else if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Safest next step: deny. Answer only after review."
    } else {
        "Safest next step: deny. Allow once only after review."
    }
}

pub(super) fn permission_modal_summary_line(
    permission: &crate::app::ActivePermissionView,
    submission_pending: bool,
) -> String {
    if submission_pending {
        return "Decision submitted — awaiting confirmation.".to_string();
    }

    permission
        .tool_label
        .as_deref()
        .map(|tool| format!("Tool {tool} is paused for review."))
        .unwrap_or_else(|| {
            if permission.summary.chars().count() > 48 {
                format!(
                    "{} request is paused for review.",
                    permission.kind.replace('_', " ")
                )
            } else {
                permission.summary.clone()
            }
        })
}

pub(super) fn permission_modal_draft_line(prompt_buffer: &str) -> String {
    let draft = prompt_buffer.trim();
    if draft.is_empty() {
        String::new()
    } else {
        format!("Draft preserved · {draft}")
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

pub(super) fn permission_modal_actions_text(
    app: &AppState,
    theme: &Theme,
    surface: Color,
    submission_pending: bool,
    is_question: bool,
) -> Text<'static> {
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);

    if submission_pending {
        return Text::from(vec![
            Line::from(vec![
                status_badge("decision sent", theme.status.info, theme),
                Span::styled("  waiting for confirmation", metadata_style),
            ]),
            Line::from(vec![Span::styled(
                "No new action required until confirmation returns.",
                metadata_style,
            )]),
        ]);
    }

    let deny_label = app.keymap.get_binding_label(Action::DenyPermission, "deny");
    let reject_label = app.keymap.get_binding_label(Action::DismissModal, "reject");
    let allow_label = app
        .keymap
        .get_binding_label(Action::AllowPermission, "allow once");

    Text::from(vec![
        Line::from(vec![
            status_badge(deny_label, theme.status.error, theme),
            Span::styled("  default deny · stays fail-closed", metadata_style),
        ]),
        Line::from(vec![
            Span::styled(
                if is_question {
                    format!("{reject_label} rejects the question")
                } else {
                    format!("{reject_label} rejects")
                },
                metadata_style,
            ),
            Span::styled("  ·  ", metadata_style),
            Span::styled(
                if is_question {
                    format!("{allow_label} sends answers")
                } else {
                    allow_label
                },
                primary_style,
            ),
        ]),
    ])
}

pub(super) fn question_permission_actions_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let confirm = !single && app.question_prompt_tab(&permission.permission_id) >= prompts.len();
    let submit_label = if confirm {
        "submit"
    } else if prompts
        .get(app.question_prompt_tab(&permission.permission_id))
        .is_some_and(|prompt| prompt.multiple)
    {
        "toggle"
    } else if single {
        "submit"
    } else {
        "confirm"
    };

    let mut spans = Vec::new();
    if !single {
        spans.push(Span::styled("⇆", primary_style));
        spans.push(Span::styled(" tab  ", metadata_style));
    }
    if !confirm {
        spans.push(Span::styled("↑↓", primary_style));
        spans.push(Span::styled(" select  ", metadata_style));
    }
    spans.push(Span::styled("enter", primary_style));
    spans.push(Span::styled(format!(" {submit_label}  "), metadata_style));
    spans.push(Span::styled("esc", primary_style));
    spans.push(Span::styled(" dismiss", metadata_style));
    Text::from(Line::from(spans))
}

pub(super) fn question_permission_body_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    if prompts.is_empty() {
        return Text::default();
    }

    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let muted_style = Style::default().fg(theme.text.secondary).bg(surface);
    let accent_style = Style::default()
        .fg(theme.text.inverse)
        .bg(ui_chrome::question_prompt_accent(theme));
    let active_surface = theme.surface.panel_elevated;
    let active_number_style = Style::default()
        .fg(question_prompt_tint(
            theme.text.secondary,
            ui_chrome::question_prompt_secondary(theme),
            0.6,
        ))
        .bg(active_surface);
    let active_label_style = Style::default()
        .fg(ui_chrome::question_prompt_secondary(theme))
        .bg(active_surface);
    let success_style = Style::default().fg(theme.status.success).bg(surface);
    let error_style = Style::default().fg(theme.status.error).bg(surface);
    let single = prompts.len() == 1 && !prompts[0].multiple;
    let tab = app
        .question_prompt_tab(&permission.permission_id)
        .min(prompts.len());
    let confirm = !single && tab >= prompts.len();
    let answers = app.question_prompt_answers(&permission.permission_id);
    let mut lines = Vec::new();

    if !single {
        let mut tabs = Vec::new();
        for (index, prompt) in prompts.iter().enumerate() {
            if index > 0 {
                tabs.push(Span::styled(" ", Style::default().bg(surface)));
            }
            let answered = answers.get(index).is_some_and(|value| !value.is_empty());
            tabs.push(Span::styled(
                format!(" {} ", prompt.header),
                if index == tab {
                    accent_style
                } else if answered {
                    primary_style
                } else {
                    muted_style
                },
            ));
        }
        tabs.push(Span::styled(" ", Style::default().bg(surface)));
        tabs.push(Span::styled(
            " Confirm ",
            if confirm { accent_style } else { muted_style },
        ));
        lines.push(Line::from(tabs));
        lines.push(Line::default());
    }

    if confirm {
        lines.push(Line::from(vec![Span::styled("Review", primary_style)]));
        for (index, prompt) in prompts.iter().enumerate() {
            let value = answers
                .get(index)
                .map(|value| value.join(", "))
                .unwrap_or_default();
            lines.push(Line::from(vec![
                Span::styled(format!("{}: ", prompt.header), muted_style),
                Span::styled(
                    if value.is_empty() {
                        "(not answered)".to_string()
                    } else {
                        value
                    },
                    if answers.get(index).is_some_and(|value| !value.is_empty()) {
                        primary_style
                    } else {
                        error_style
                    },
                ),
            ]));
        }
        return Text::from(lines);
    }

    let prompt = &prompts[tab.min(prompts.len().saturating_sub(1))];
    let selected = app.question_prompt_selection(&permission.permission_id);
    let current_answers = answers.get(tab).cloned().unwrap_or_default();

    lines.push(Line::from(vec![Span::styled(
        if prompt.multiple {
            format!("{} (select all that apply)", prompt.question)
        } else {
            prompt.question.clone()
        },
        primary_style,
    )]));
    lines.push(Line::default());

    for (index, option) in prompt.options.iter().enumerate() {
        let picked = current_answers.iter().any(|value| value == &option.label);
        let active = index == selected;
        let mut row = vec![Span::styled(
            format!("{}.", index + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] {}", if picked { '✓' } else { ' ' }, option.label)
            } else {
                option.label.clone()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));
        lines.push(Line::from(vec![Span::styled(
            format!("   {}", option.description),
            muted_style,
        )]));
    }

    if prompt.custom {
        let custom_value = app
            .question_prompt_custom(&permission.permission_id, tab)
            .unwrap_or_default();
        let picked =
            !custom_value.is_empty() && current_answers.iter().any(|value| value == custom_value);
        let active = selected == prompt.options.len();
        let mut row = vec![Span::styled(
            format!("{}.", prompt.options.len() + 1),
            if active {
                active_number_style
            } else {
                muted_style
            },
        )];
        row.push(Span::styled(
            " ",
            Style::default().bg(if active { active_surface } else { surface }),
        ));
        row.push(Span::styled(
            if prompt.multiple {
                format!("[{}] Type your own answer", if picked { '✓' } else { ' ' })
            } else {
                "Type your own answer".to_string()
            },
            if active {
                active_label_style
            } else if prompt.multiple && picked {
                success_style
            } else {
                primary_style
            },
        ));
        if !prompt.multiple {
            row.push(Span::styled(if picked { "✓" } else { "" }, success_style));
        }
        lines.push(Line::from(row));

        let editing = app.question_prompt_editing(&permission.permission_id) && active;
        if editing {
            let preview = app.question_answer_preview(&permission.permission_id);
            let (text, style) = if preview == "█" {
                ("Type your own answer".to_string(), muted_style)
            } else {
                (preview, primary_style)
            };
            lines.push(Line::from(vec![Span::styled(format!("   {text}"), style)]));
        } else if !custom_value.is_empty() {
            lines.push(Line::from(vec![Span::styled(
                format!("   {custom_value}"),
                muted_style,
            )]));
        }
    }

    if let Some(error) = app.question_answer_error(&permission.permission_id) {
        lines.push(Line::default());
        lines.push(Line::from(vec![Span::styled(
            error.to_string(),
            error_style,
        )]));
    }

    Text::from(lines)
}

fn question_prompt_tint(base: Color, overlay: Color, alpha: f32) -> Color {
    match (base, overlay) {
        (
            Color::Rgb(base_red, base_green, base_blue),
            Color::Rgb(overlay_red, overlay_green, overlay_blue),
        ) => {
            let blend = |base: u8, overlay: u8| -> u8 {
                let value = (f32::from(base) * (1.0 - alpha)) + (f32::from(overlay) * alpha);
                value.round().clamp(0.0, 255.0) as u8
            };
            Color::Rgb(
                blend(base_red, overlay_red),
                blend(base_green, overlay_green),
                blend(base_blue, overlay_blue),
            )
        }
        _ => overlay,
    }
}

#[cfg(test)]
mod fork_selector_tests {
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
