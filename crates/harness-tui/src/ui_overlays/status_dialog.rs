// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::Path;

use super::*;

use crate::text::trimmed_json_nested_string_field;

pub(super) fn render_status_dialog_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
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
    status_dialog_body_from_rows(status_dialog_mcp_rows(), status_dialog_lsp_rows(app), theme)
}

fn status_dialog_body_from_rows(
    mcp_rows: Vec<StatusDialogRow>,
    lsp_rows: Vec<StatusDialogRow>,
    theme: &Theme,
) -> Text<'static> {
    let mut lines = Vec::new();
    append_status_dialog_mcp_section(&mut lines, mcp_rows, theme);
    append_status_dialog_lsp_section(&mut lines, lsp_rows, theme);
    append_status_dialog_formatters_section(
        &mut lines,
        theme,
        harness_core::config::registered_formatter_config().as_ref(),
    );
    append_status_dialog_plugins_section(&mut lines, theme);
    Text::from(lines)
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

fn append_status_dialog_formatters_section(
    lines: &mut Vec<Line<'static>>,
    theme: &Theme,
    formatter_config: Option<&harness_core::config::FormatterConfig>,
) {
    let Some(formatter_config) = formatter_config else {
        lines.push(status_dialog_row_line(
            StatusDialogRow {
                name: "disabled".to_string(),
                suffix: None,
                tone: StatusDialogTone::Muted,
                enabled: false,
            },
            theme,
        ));
        lines.push(Line::default());
        return;
    };

    if !formatter_config.enabled {
        lines.push(status_dialog_row_line(
            StatusDialogRow {
                name: "disabled".to_string(),
                suffix: None,
                tone: StatusDialogTone::Muted,
                enabled: false,
            },
            theme,
        ));
        lines.push(Line::default());
        return;
    }

    if formatter_config.overrides.is_empty() {
        lines.push(status_dialog_row_line(
            StatusDialogRow {
                name: "auto-detect".to_string(),
                suffix: None,
                tone: StatusDialogTone::Muted,
                enabled: true,
            },
            theme,
        ));
        lines.push(Line::default());
        return;
    }

    for override_name in formatter_config.overrides.keys() {
        let display_name = override_name
            .strip_prefix("_lang_")
            .unwrap_or(override_name);
        lines.push(status_dialog_row_line(
            StatusDialogRow {
                name: display_name.to_string(),
                suffix: Some("configured".to_string()),
                tone: StatusDialogTone::Success,
                enabled: true,
            },
            theme,
        ));
    }
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
pub(crate) fn exact_test_status_dialog_formatters_section_disabled_when_none() {
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_formatters_section(&mut lines, &theme, None);
    assert_eq!(lines.len(), 2);
    let row = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone())
        .collect::<String>();
    assert!(row.contains("disabled"), "expected disabled label: {row}");
}

#[cfg(test)]
pub(crate) fn exact_test_status_dialog_formatters_section_lists_enabled_language() {
    let theme = Theme::default();
    let mut config = harness_core::config::FormatterConfig {
        enabled: true,
        ..Default::default()
    };
    config.overrides.insert(
        "rust".to_string(),
        harness_core::config::FormatterOverride {
            disabled: false,
            command: Some(vec!["rustfmt".to_string()]),
            environment: None,
            extensions: Some(vec![".rs".to_string()]),
        },
    );

    let mut lines = Vec::new();
    append_status_dialog_formatters_section(&mut lines, &theme, Some(&config));

    assert_eq!(lines.len(), 2);
    let row = lines[0]
        .spans
        .iter()
        .map(|span| span.content.clone())
        .collect::<String>();
    assert!(row.contains("rust"), "expected rust language label: {row}");
    assert!(
        row.contains("configured"),
        "expected configured suffix: {row}"
    );
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
    let mut terminal = ratatui::Terminal::new(backend).unwrap_or_abort();
    terminal
        .draw(|frame| {
            let root = Rect::new(0, 0, 80, 24);
            let overlay = status_dialog_area(root).unwrap_or_abort();
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
                status_dialog_body_from_rows(mcp_rows, lsp_rows, &theme),
            );
        })
        .unwrap_or_abort();

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
