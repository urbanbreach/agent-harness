// allow: SIZE_OK — TUI overlay rendering (indivisible view model)
use crate::UnwrapOrAbort;
use std::collections::BTreeMap;
use std::path::Path;

use harness_core::event::EventV1;

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
    if let Some(dashboard) = app.status_dashboard() {
        render_interactive_dashboard(frame, app, theme, overlay);
        let summary_area = Rect::new(
            chunks[1].x.saturating_add(1),
            chunks[1].bottom().saturating_sub(8),
            chunks[1].width.saturating_sub(2),
            7,
        );
        render_dashboard_summary(frame, app, theme, summary_area);
        if dashboard.help_visible() {
            render_dashboard_help(frame, theme, overlay, dashboard);
        }
    } else {
        render_status_dialog_body(frame, theme, chunks[1], status_dialog_body(app, theme));
    }
    render_status_dialog_header(frame, theme, chunks[0]);
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
    crate::dashboard_integration::dashboard_viewport(root)
}

fn render_status_dialog_header(frame: &mut Frame, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let surface = ui_chrome::command_palette_surface(theme);
    let title = "Status · Harness dashboard";
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

fn render_interactive_dashboard(frame: &mut Frame, app: &AppState, theme: &Theme, overlay: Rect) {
    let Some(dashboard) = app.status_dashboard() else {
        return;
    };
    let layout = dashboard.layout();
    render_dashboard_roster(frame, theme, layout.roster, dashboard);
    render_dashboard_peek(frame, theme, layout.peek, dashboard);
    render_dashboard_reply(frame, theme, layout.reply, dashboard);
    if let Some(details) = layout.details {
        render_dashboard_details(frame, theme, details, dashboard);
    }
    let focus = format!(
        "focus: {:?} · Tab focus · / search · h help · esc close",
        dashboard.focus()
    );
    let footer = Rect::new(
        overlay.x.saturating_add(2),
        overlay.bottom().saturating_sub(2),
        overlay.width.saturating_sub(4),
        1,
    );
    frame.render_widget(
        Paragraph::new(focus).style(Style::default().fg(theme.text.secondary)),
        footer,
    );
}

fn render_dashboard_roster(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let layout = dashboard.roster_layout();
    let lines = layout
        .rows
        .iter()
        .map(|row| {
            let data = dashboard.dashboard().row(row.selection_key.as_str());
            let status = data.map_or("unknown", |entry| dashboard_status_label(entry.status));
            let marker = if row.selected { ">" } else { " " };
            format!("{marker} {status:<9} {}", row.label)
        })
        .collect::<Vec<_>>();
    render_dashboard_pane(frame, theme, area, "Roster", lines);
}

fn render_dashboard_peek(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let lines = match dashboard.peek_view() {
        Ok(view) => {
            let mut lines = vec![
                format!("session: {}", view.session_id.as_str()),
                format!("tail: {} blocks", view.blocks.len()),
                format!("unread: {}", view.unread_count),
                format!("follow: {:?}", view.follow),
            ];
            if !view.draft.is_empty() {
                lines.push(format!("draft: {}", view.draft));
            }
            lines.extend(view.blocks.into_iter().map(|block| block.content));
            lines
        }
        Err(error) => vec![error.to_string()],
    };
    render_dashboard_pane(frame, theme, area, "Peek / tail", lines);
}

fn render_dashboard_reply(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let visual = dashboard.controls_visual();
    let mut lines = vec![
        "reply composer".to_string(),
        format!("controls: {}", visual.state.label()),
    ];
    if let Some(message) = visual.message.as_deref() {
        lines.push(message.to_string());
    }
    if let Ok(view) = dashboard.peek_view() {
        lines.push(if view.draft.is_empty() {
            "draft: <empty>".to_string()
        } else {
            format!("draft: {}", view.draft)
        });
    }
    render_dashboard_pane(frame, theme, area, "Reply", lines);
}

fn render_dashboard_details(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let lines = match dashboard.details_fields() {
        Ok(fields) => vec![
            format!("id: {}", fields.session_id.as_str()),
            format!("status: {}", dashboard_status_label(fields.status)),
            format!("title: {}", fields.title.unwrap_or_default()),
            format!(
                "provider: {}",
                fields.metadata.provider_model.unwrap_or_default()
            ),
            format!(
                "parent: {}",
                fields
                    .parent
                    .map_or_else(|| "none".to_string(), |id| id.as_str().to_string())
            ),
            format!("children: {}", fields.children.len()),
        ],
        Err(error) => vec![error.to_string()],
    };
    render_dashboard_pane(frame, theme, area, "Details", lines);
}

fn render_dashboard_pane(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    lines: Vec<String>,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border.subtle))
        .title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .style(Style::default().fg(theme.text.primary))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn render_dashboard_summary(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let plugins = status_dialog_plugin_summary(app);
    let edits = status_dialog_edit_attribution_summary(app);
    let operator = status_dialog_operator_summary(app);
    let mcp = if status_dialog_mcp_rows().is_empty() {
        "No MCP Servers".to_string()
    } else {
        "MCP servers available".to_string()
    };
    let plugin_line = format!(
        "Plugins: {} installed ({} enabled, {} disabled)",
        plugins.installed, plugins.enabled, plugins.disabled
    );
    let edit_line = if edits.total == 0 {
        "Edit attribution: none yet".to_string()
    } else {
        format!("Edit attribution: {} edits", edits.total)
    };
    let operator_line = operator.dashboard_one_line();
    let crash_line = operator.crash_or_recovery.map_or_else(
        || "Crash/recovery: none".to_string(),
        |value| format!("Crash/recovery: {value}"),
    );
    let fallback_line = operator
        .fallback_banner
        .map_or_else(String::new, |value| format!("Fallback banner: {value}"));
    let mut lines = vec![
        "Operator".to_string(),
        mcp,
        plugin_line,
        edit_line,
        operator_line,
        crash_line,
    ];
    if !fallback_line.is_empty() {
        lines.push(fallback_line);
    }
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .style(Style::default().fg(theme.text.secondary))
            .wrap(Wrap { trim: true }),
        area,
    );
}

fn render_dashboard_help(
    frame: &mut Frame,
    theme: &Theme,
    overlay: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let help = dashboard.help(crate::app::Focus::List);
    let area = Rect::new(
        overlay.x.saturating_add(4),
        overlay.y.saturating_add(3),
        overlay.width.saturating_sub(8),
        overlay.height.saturating_sub(6),
    );
    let lines = help
        .entries
        .into_iter()
        .map(|entry| format!("{}  {}", entry.key, entry.action))
        .collect::<Vec<_>>();
    render_dashboard_pane(frame, theme, area, "Dashboard help", lines);
}

fn dashboard_status_label(status: crate::dashboard::DashboardStatus) -> &'static str {
    match status {
        crate::dashboard::DashboardStatus::Running => "working",
        crate::dashboard::DashboardStatus::Queued => "queued",
        crate::dashboard::DashboardStatus::Streaming => "streaming",
        crate::dashboard::DashboardStatus::Completed => "settled",
        crate::dashboard::DashboardStatus::Failed => "failed",
        crate::dashboard::DashboardStatus::Cancelled => "stopped",
        crate::dashboard::DashboardStatus::Stale => "stale",
    }
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
        status_dialog_mcp_rows(),
        status_dialog_lsp_rows(app),
        status_dialog_plugin_summary(app),
        status_dialog_edit_attribution_summary(app),
        status_dialog_operator_summary(app),
        theme,
    )
}

fn status_dialog_body_from_rows(
    mcp_rows: Vec<StatusDialogRow>,
    lsp_rows: Vec<StatusDialogRow>,
    plugin_summary: PluginDialogSummary,
    edit_summary: EditAttributionDialogSummary,
    operator_summary: OperatorDialogSummary,
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
    append_status_dialog_plugins_section(&mut lines, plugin_summary, theme);
    append_status_dialog_edit_attribution_section(&mut lines, edit_summary, theme);
    append_status_dialog_operator_section(&mut lines, operator_summary, theme);
    Text::from(lines)
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct PluginDialogSummary {
    installed: usize,
    enabled: usize,
    disabled: usize,
    extension_descriptor: Option<String>,
    last_install: Option<String>,
    last_activate: Option<String>,
    last_deactivate: Option<String>,
    last_remove: Option<String>,
    first_plugin: Option<String>,
    discover: Option<String>,
    last_load: Option<String>,
}

fn status_dialog_plugin_summary(app: &AppState) -> PluginDialogSummary {
    let extension_descriptor = app.extension_manifest_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let last_install = app.plugin_last_install().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let last_activate = app.plugin_last_activate().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let last_deactivate = app.plugin_last_deactivate().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let last_remove = app.plugin_last_remove().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let first_plugin = app.plugin_first_line().map(sanitize_status_dialog_text);
    let discover = app.extension_discover_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let last_load = app.extension_last_load().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    match app.plugin_lifecycle_summary() {
        Some(summary) => PluginDialogSummary {
            installed: summary.installed,
            enabled: summary.enabled,
            disabled: summary.disabled,
            extension_descriptor,
            last_install,
            last_activate,
            last_deactivate,
            last_remove,
            first_plugin,
            discover,
            last_load,
        },
        None => PluginDialogSummary {
            extension_descriptor,
            last_install,
            last_activate,
            last_deactivate,
            last_remove,
            first_plugin,
            discover,
            last_load,
            ..PluginDialogSummary::default()
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct EditAttributionDialogSummary {
    agent_tool: usize,
    external: usize,
    total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct OperatorDialogSummary {
    crash_or_recovery: Option<String>,
    provider_fallback: Option<String>,
    fallback_chain: Option<String>,
    fallback_outcome: Option<String>,
    fallback_banner: Option<String>,
    fallback_models: Option<String>,
    demote_handle: Option<String>,
    demote_outcomes: Option<String>,
    demote_last: Option<String>,
    demote_last_task: Option<String>,
    settings_bound: bool,
    settings_writable_paths: usize,
    settings_editable: usize,
    settings_total: usize,
    settings_registry: Option<String>,
    crash_scan: Option<String>,
    crash_recovery_next: Option<String>,
    crash_recovery_action: Option<String>,
    crash_recovery_report: Option<String>,
    teams: Option<String>,
    team_last_create: Option<String>,
    team_first: Option<String>,
    team_last_send: Option<String>,
    team_last_message: Option<String>,
    team_add_member: Option<String>,
    team_cancel: Option<String>,
    cron: Option<String>,
    cron_last_register: Option<String>,
    cron_first_schedule: Option<String>,
    cron_last_remove: Option<String>,
    workspace_hub: Option<String>,
    workspace_hub_availability: Option<String>,
    workspace_hub_connect: Option<String>,
    workspace_hub_bind: Option<String>,
    workspace_hub_upload: Option<String>,
    workspace_hub_recover: Option<String>,
    graph_batch: Option<String>,
    graph_query_last: Option<String>,
    graph_batch_first: Option<String>,
    persistent_graph: Option<String>,
    cow_clone: Option<String>,
    cow_clone_last: Option<String>,
    cow_fastpath: Option<String>,
    browser_oidc: Option<String>,
    browser_oidc_availability: Option<String>,
    browser_oidc_start: Option<String>,
    browser_oidc_complete: Option<String>,
    mcp_oauth: Option<String>,
    mcp_oauth_remote_availability: Option<String>,
    mcp_oauth_begin: Option<String>,
    mcp_oauth_exchange: Option<String>,
    mcp_oauth_open: Option<String>,
    sleep_wake: Option<String>,
    sleep_wake_policy: Option<String>,
    sleep_wake_last: Option<String>,
    sleep_wake_decision: Option<String>,
    sleep_wake_availability: Option<String>,
    binary_update: Option<String>,
    binary_update_policy: Option<String>,
    binary_update_check: Option<String>,
    binary_version: Option<String>,
    foreign_discover: Option<String>,
    foreign_import_next: Option<String>,
    foreign_import_last: Option<String>,
    jujutsu: Option<String>,
    jujutsu_cli: Option<String>,
    jujutsu_workspace: Option<String>,
    jujutsu_last_command: Option<String>,
    sandbox: Option<String>,
    landlock: Option<String>,
    os_sandbox_profiles: Option<String>,
    os_sandbox_first_profile: Option<String>,
    sandbox_last_prepare: Option<String>,
    acp: Option<String>,
    acp_state: Option<String>,
    acp_session: Option<String>,
    acp_last_connect: Option<String>,
    acp_last_bind: Option<String>,
    edit_attribution: Option<String>,
    edit_attribution_first: Option<String>,
    edit_attribution_last: Option<String>,
    plan_view: Option<String>,
    plan_view_first: Option<String>,
}

impl OperatorDialogSummary {
    /// Count optional operator probe fields that are currently bound (Some),
    /// plus the settings project-config bind surface.
    fn bound_probe_counts(&self) -> (usize, usize) {
        let probes = [
            self.crash_or_recovery.is_some(),
            self.provider_fallback.is_some(),
            self.fallback_chain.is_some(),
            self.fallback_outcome.is_some(),
            self.fallback_banner.is_some(),
            self.fallback_models.is_some(),
            self.demote_handle.is_some(),
            self.demote_outcomes.is_some(),
            self.demote_last.is_some(),
            self.demote_last_task.is_some(),
            self.crash_scan.is_some(),
            self.crash_recovery_next.is_some(),
            self.crash_recovery_action.is_some(),
            self.crash_recovery_report.is_some(),
            self.teams.is_some(),
            self.team_last_create.is_some(),
            self.team_first.is_some(),
            self.team_last_send.is_some(),
            self.team_last_message.is_some(),
            self.team_add_member.is_some(),
            self.team_cancel.is_some(),
            self.cron.is_some(),
            self.cron_last_register.is_some(),
            self.cron_first_schedule.is_some(),
            self.cron_last_remove.is_some(),
            self.workspace_hub.is_some(),
            self.workspace_hub_availability.is_some(),
            self.workspace_hub_connect.is_some(),
            self.workspace_hub_bind.is_some(),
            self.workspace_hub_upload.is_some(),
            self.workspace_hub_recover.is_some(),
            self.graph_batch.is_some(),
            self.graph_query_last.is_some(),
            self.graph_batch_first.is_some(),
            self.persistent_graph.is_some(),
            self.cow_clone.is_some(),
            self.cow_clone_last.is_some(),
            self.cow_fastpath.is_some(),
            self.browser_oidc.is_some(),
            self.browser_oidc_availability.is_some(),
            self.browser_oidc_start.is_some(),
            self.browser_oidc_complete.is_some(),
            self.mcp_oauth.is_some(),
            self.mcp_oauth_remote_availability.is_some(),
            self.mcp_oauth_begin.is_some(),
            self.mcp_oauth_exchange.is_some(),
            self.mcp_oauth_open.is_some(),
            self.sleep_wake.is_some(),
            self.sleep_wake_policy.is_some(),
            self.sleep_wake_last.is_some(),
            self.sleep_wake_decision.is_some(),
            self.sleep_wake_availability.is_some(),
            self.binary_update.is_some(),
            self.binary_update_policy.is_some(),
            self.binary_update_check.is_some(),
            self.binary_version.is_some(),
            self.foreign_discover.is_some(),
            self.foreign_import_next.is_some(),
            self.foreign_import_last.is_some(),
            self.jujutsu.is_some(),
            self.jujutsu_cli.is_some(),
            self.jujutsu_workspace.is_some(),
            self.jujutsu_last_command.is_some(),
            self.sandbox.is_some(),
            self.landlock.is_some(),
            self.os_sandbox_profiles.is_some(),
            self.os_sandbox_first_profile.is_some(),
            self.sandbox_last_prepare.is_some(),
            self.acp.is_some(),
            self.acp_state.is_some(),
            self.acp_session.is_some(),
            self.acp_last_connect.is_some(),
            self.acp_last_bind.is_some(),
            self.edit_attribution.is_some(),
            self.edit_attribution_first.is_some(),
            self.edit_attribution_last.is_some(),
            self.plan_view.is_some(),
            self.plan_view_first.is_some(),
            self.settings_registry.is_some(),
        ];
        let mut bound = probes.iter().filter(|b| **b).count();
        let mut total = probes.len();
        total = total.saturating_add(1);
        if self.settings_bound {
            bound = bound.saturating_add(1);
        }
        (bound, total)
    }

    fn dashboard_one_line(&self) -> String {
        let (bound, total) = self.bound_probe_counts();
        format!("operator dashboard: {bound} bound of {total} probes")
    }
}

fn status_dialog_edit_attribution_summary(app: &AppState) -> EditAttributionDialogSummary {
    let mut applied: BTreeMap<String, String> = BTreeMap::new();
    for event in &app.events {
        if let EventV1::EditApplied(edit) = &event.payload {
            applied.insert(edit.path.clone(), edit.new_file_digest.clone());
        }
    }
    if applied.is_empty() {
        return EditAttributionDialogSummary::default();
    }

    let Some(workspace_root) = app.file_mention_workspace_root_opt() else {
        let agent_tool = applied.len();
        return EditAttributionDialogSummary {
            agent_tool,
            external: 0,
            total: agent_tool,
        };
    };

    let mut agent_tool = 0usize;
    let mut external = 0usize;
    for (rel_path, expected_digest) in &applied {
        let candidate = {
            let input = Path::new(rel_path);
            if input.is_absolute() {
                input.to_path_buf()
            } else {
                workspace_root.join(input)
            }
        };
        match harness_core::edit_attribution::path_content_digest12(&candidate) {
            Ok(actual) if actual == *expected_digest => agent_tool += 1,
            Ok(_) => external += 1,
            Err(_) => agent_tool += 1,
        }
    }
    EditAttributionDialogSummary {
        agent_tool,
        external,
        total: agent_tool.saturating_add(external),
    }
}

fn status_dialog_operator_summary(app: &AppState) -> OperatorDialogSummary {
    let banner = app
        .status_banner
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let provider_fallback = banner
        .filter(|text| text.to_ascii_lowercase().contains("provider fallback"))
        .map(sanitize_status_dialog_text);
    let crash_or_recovery = banner
        .filter(|text| {
            let lower = text.to_ascii_lowercase();
            lower.contains("previous crash")
                || lower.contains("recovery")
                || lower.contains("action:")
                || lower.contains("stale writer")
        })
        .filter(|_| provider_fallback.is_none())
        .map(sanitize_status_dialog_text);
    let demote_handle = app.focused_demote_handle_id().map(|id| {
        let cleaned = sanitize_status_dialog_text(&id);
        if cleaned.chars().count() > 48 {
            cleaned.chars().take(45).collect::<String>() + "..."
        } else {
            cleaned
        }
    });
    let settings = app.settings_editor_summary();
    let settings_registry = app.settings_registry_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let crash_scan = app.crash_recovery_scan_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let crash_recovery_next = app.crash_recovery_first_report().map(|report| {
        let run_id = report
            .run_dir
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("session");
        let line = match report.recovery_action {
            Some(action) => format!(
                "{} · next: {}",
                report.one_line(),
                action.operator_hint(run_id)
            ),
            None => report.one_line(),
        };
        sanitize_status_dialog_text(&line)
    });
    let crash_recovery_action = app.crash_recovery_resolved_action().map(|action| {
        let run_id = app
            .crash_recovery_first_report()
            .and_then(|report| report.run_dir.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("(session)");
        let line = format!("{} — {}", action.as_str(), action.operator_hint(run_id));
        sanitize_status_dialog_text(&line)
    });
    let crash_recovery_report = app
        .crash_recovery_first_report_line()
        .map(sanitize_status_dialog_text);
    let teams = app.team_registry_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let team_last_create = app.team_last_create().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let team_first = app.team_first_line().map(sanitize_status_dialog_text);
    let team_last_send = app.team_last_send().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let team_last_message = app
        .team_last_message_line()
        .map(sanitize_status_dialog_text);
    let team_add_member = app.team_last_add_member().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let team_cancel = app.team_last_cancel().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cron = app.cron_schedule_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cron_last_register = app.cron_last_register().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cron_first_schedule = app
        .cron_first_schedule_line()
        .map(sanitize_status_dialog_text);
    let cron_last_remove = app.cron_last_remove().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let demote_outcomes = app.demote_outcome_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let demote_last = app.demote_last_result().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let demote_last_task = app.demote_last_task_result().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let fallback_chain = app.auto_fallback_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let fallback_outcome = app.auto_fallback_last_outcome().map(|outcome| {
        let line = harness_core::auto_fallback::describe_auto_fallback_outcome(outcome);
        sanitize_status_dialog_text(&line)
    });
    let fallback_banner = app
        .auto_fallback_last_banner()
        .map(sanitize_status_dialog_text);
    let fallback_models = app
        .auto_fallback_chain_label()
        .map(sanitize_status_dialog_text);
    let workspace_hub = app.workspace_hub_outcome_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let workspace_hub_availability = app.workspace_hub_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let workspace_hub_connect = app.workspace_hub_last_connect().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let workspace_hub_bind = app.workspace_hub_last_bind().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let workspace_hub_upload = app.workspace_hub_last_upload().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let workspace_hub_recover = app.workspace_hub_last_recover().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let graph_batch = app.graph_query_batch_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let graph_query_last = app.graph_query_last_result().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let graph_batch_first = app
        .graph_query_batch_first_line()
        .map(sanitize_status_dialog_text);
    let persistent_graph = app.persistent_graph_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cow_clone = app.cow_clone_outcome_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cow_clone_last = app.cow_clone_last_result().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let cow_fastpath = app.cow_worktree_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let browser_oidc = app.browser_oidc_outcome_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let browser_oidc_availability = app.browser_oidc_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let browser_oidc_start = app.browser_oidc_last_start().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let browser_oidc_complete = app.browser_oidc_last_complete().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let mcp_oauth = app.mcp_oauth_outcome_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let mcp_oauth_remote_availability = app.mcp_oauth_remote_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let mcp_oauth_begin = app.mcp_oauth_last_begin().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let mcp_oauth_exchange = app.mcp_oauth_last_exchange().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let mcp_oauth_open = app.mcp_oauth_last_open().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sleep_wake = app.sleep_wake_observation_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sleep_wake_policy = app.sleep_wake_credential_policy().map(|policy| {
        let line = policy.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sleep_wake_last = app.sleep_wake_last_observation().map(|observation| {
        let line = observation.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sleep_wake_decision = app.sleep_wake_last_decision().map(|decision| {
        let line = decision.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sleep_wake_availability = app.sleep_wake_availability().map(|availability| {
        let line = availability.one_line();
        sanitize_status_dialog_text(&line)
    });
    let binary_update = app.binary_update_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let binary_update_policy = app.binary_update_policy().map(|policy| {
        let line = policy.one_line();
        sanitize_status_dialog_text(&line)
    });
    let binary_update_check = app.binary_update_check().map(|check| {
        let line = check.one_line();
        sanitize_status_dialog_text(&line)
    });
    let binary_version = app.binary_version_info().map(|info| {
        let line = info.one_line();
        sanitize_status_dialog_text(&line)
    });
    let foreign_discover = app.foreign_discover_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let foreign_import_next = app.foreign_import_first_candidate().map(|candidate| {
        let path = candidate.path().display().to_string();
        let line = match candidate {
            harness_core::foreign_session::ForeignSessionCandidate::Discoverable {
                kind,
                marker,
                ..
            } => format!(
                "importable {} ({}) · next: harness sessions import-foreign --from {}",
                kind.as_str(),
                marker,
                path
            ),
            _ => format!("candidate · path={path}"),
        };
        sanitize_status_dialog_text(&line)
    });
    let foreign_import_last = app.foreign_import_last_outcome().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let jujutsu = app.jujutsu_probe().map(|probe| {
        let line = probe.one_line();
        sanitize_status_dialog_text(&line)
    });
    let jujutsu_cli = app.jujutsu_cli().map(|cli| {
        let line = cli.one_line();
        sanitize_status_dialog_text(&line)
    });
    let jujutsu_workspace = app.jujutsu_workspace().map(|workspace| {
        let line = workspace.one_line();
        sanitize_status_dialog_text(&line)
    });
    let jujutsu_last_command = app.jujutsu_last_command().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let sandbox = app.sandbox_fs_plan_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let landlock = app.landlock_support().map(|support| {
        let line = support.one_line();
        sanitize_status_dialog_text(&line)
    });
    let os_sandbox_profiles = app.os_sandbox_profiles_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let os_sandbox_first_profile = app
        .os_sandbox_first_profile_line()
        .map(sanitize_status_dialog_text);
    let sandbox_last_prepare = app.sandbox_last_prepare().map(|result| {
        let line = result.one_line();
        sanitize_status_dialog_text(&line)
    });
    let acp = app.acp_connection_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let acp_state = app.acp_connection_state().map(|state| {
        let line = state.one_line();
        sanitize_status_dialog_text(&line)
    });
    let acp_session = app.acp_session_info().map(|session| {
        let line = session.one_line();
        sanitize_status_dialog_text(&line)
    });
    let acp_last_connect = app.acp_last_connect().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let acp_last_bind = app.acp_last_bind().map(|outcome| {
        let line = outcome.one_line();
        sanitize_status_dialog_text(&line)
    });
    let edit_attribution = app.edit_attribution_summary().map(|summary| {
        let line = summary.one_line();
        sanitize_status_dialog_text(&line)
    });
    let edit_attribution_first = app
        .edit_attribution_first_line()
        .map(sanitize_status_dialog_text);
    let edit_attribution_last = app
        .edit_attribution_last_line()
        .map(sanitize_status_dialog_text);
    let plan_view = {
        let summary = app.plan_view_summary();
        let line = summary.one_line();
        Some(sanitize_status_dialog_text(&line))
    };
    let plan_view_first = app.plan_view_rows().into_iter().next().map(|row| {
        let line = row.one_line();
        sanitize_status_dialog_text(&line)
    });
    OperatorDialogSummary {
        crash_or_recovery,
        provider_fallback,
        fallback_chain,
        fallback_outcome,
        fallback_banner,
        fallback_models,
        demote_handle,
        demote_outcomes,
        demote_last,
        demote_last_task,
        settings_bound: settings.bound,
        settings_writable_paths: settings.writable_paths,
        settings_editable: settings.editable,
        settings_total: settings.total,
        settings_registry,
        crash_scan,
        crash_recovery_next,
        crash_recovery_action,
        crash_recovery_report,
        teams,
        team_last_create,
        team_first,
        team_last_send,
        team_last_message,
        team_add_member,
        team_cancel,
        cron,
        cron_last_register,
        cron_first_schedule,
        cron_last_remove,
        workspace_hub,
        workspace_hub_availability,
        workspace_hub_connect,
        workspace_hub_bind,
        workspace_hub_upload,
        workspace_hub_recover,
        graph_batch,
        graph_query_last,
        graph_batch_first,
        persistent_graph,
        cow_clone,
        cow_clone_last,
        cow_fastpath,
        browser_oidc,
        browser_oidc_availability,
        browser_oidc_start,
        browser_oidc_complete,
        mcp_oauth,
        mcp_oauth_remote_availability,
        mcp_oauth_begin,
        mcp_oauth_exchange,
        mcp_oauth_open,
        sleep_wake,
        sleep_wake_policy,
        sleep_wake_last,
        sleep_wake_decision,
        sleep_wake_availability,
        binary_update,
        binary_update_policy,
        binary_update_check,
        binary_version,
        foreign_discover,
        foreign_import_next,
        foreign_import_last,
        jujutsu,
        jujutsu_cli,
        jujutsu_workspace,
        jujutsu_last_command,
        sandbox,
        landlock,
        os_sandbox_profiles,
        os_sandbox_first_profile,
        sandbox_last_prepare,
        acp,
        acp_state,
        acp_session,
        acp_last_connect,
        acp_last_bind,
        edit_attribution,
        edit_attribution_first,
        edit_attribution_last,
        plan_view,
        plan_view_first,
    }
}

fn append_status_dialog_edit_attribution_section(
    lines: &mut Vec<Line<'static>>,
    summary: EditAttributionDialogSummary,
    theme: &Theme,
) {
    if summary.total == 0 {
        lines.push(status_dialog_plain_line(
            "Edit attribution: none yet",
            theme,
        ));
        return;
    }
    lines.push(status_dialog_plain_line(
        format!(
            "Edit attribution: {} agent-tool, {} external",
            summary.agent_tool, summary.external
        ),
        theme,
    ));
}

fn append_status_dialog_operator_section(
    lines: &mut Vec<Line<'static>>,
    summary: OperatorDialogSummary,
    theme: &Theme,
) {
    lines.push(Line::default());
    lines.push(status_dialog_plain_line("Operator", theme));
    lines.push(status_dialog_plain_line(
        summary.dashboard_one_line(),
        theme,
    ));
    match summary.crash_or_recovery.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Crash/recovery: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Crash/recovery: none", theme)),
    }
    match summary.provider_fallback.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Fallback: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Fallback: none", theme)),
    }
    match summary.fallback_chain.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Fallback chain: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Fallback chain: none", theme)),
    }
    match summary.fallback_outcome.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Fallback outcome: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Fallback outcome: none", theme)),
    }
    match summary.fallback_banner.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Fallback banner: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Fallback banner: none", theme)),
    }
    match summary.fallback_models.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Fallback models: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Fallback models: none", theme)),
    }
    match summary.demote_handle.as_deref() {
        Some(handle) => lines.push(status_dialog_plain_line(
            format!("Demote focus: {handle}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Demote focus: none (Ctrl+B bulk when available)",
            theme,
        )),
    }
    match summary.demote_outcomes.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Demote outcomes: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Demote outcomes: none", theme)),
    }
    match summary.demote_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Demote last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Demote last: none", theme)),
    }
    match summary.demote_last_task.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Demote last task: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Demote last task: none", theme)),
    }
    let settings_line = if summary.settings_bound {
        format!(
            "Settings: bound, {}/{} writable editable (registry {})",
            summary.settings_editable, summary.settings_writable_paths, summary.settings_total
        )
    } else {
        format!(
            "Settings: unbound, {} write paths (registry {})",
            summary.settings_writable_paths, summary.settings_total
        )
    };
    lines.push(status_dialog_plain_line(settings_line, theme));
    match summary.settings_registry.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Settings registry: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Settings registry: none", theme)),
    }
    match summary.crash_scan.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Crash scan: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Crash scan: none", theme)),
    }
    match summary.crash_recovery_next.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Crash recovery: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Crash recovery: none", theme)),
    }
    match summary.crash_recovery_action.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Crash recovery action: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Crash recovery action: none",
            theme,
        )),
    }
    match summary.crash_recovery_report.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Crash recovery report: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Crash recovery report: none",
            theme,
        )),
    }
    match summary.teams.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Teams: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Teams: none", theme)),
    }
    match summary.team_last_create.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team create: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team create: none", theme)),
    }
    match summary.team_first.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team first: none", theme)),
    }
    match summary.team_last_send.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team send: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team send: none", theme)),
    }
    match summary.team_last_message.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team mailbox last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team mailbox last: none", theme)),
    }
    match summary.team_add_member.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team add-member: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team add-member: none", theme)),
    }
    match summary.team_cancel.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Team cancel: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Team cancel: none", theme)),
    }
    match summary.cron.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Cron: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Cron: none", theme)),
    }
    match summary.cron_last_register.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Cron register: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Cron register: none", theme)),
    }
    match summary.cron_first_schedule.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Cron first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Cron first: none", theme)),
    }
    match summary.cron_last_remove.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Cron remove: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Cron remove: none", theme)),
    }
    match summary.workspace_hub.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Workspace hub: none", theme)),
    }
    match summary.workspace_hub_availability.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub availability: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Workspace hub availability: none",
            theme,
        )),
    }
    match summary.workspace_hub_connect.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub connect: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Workspace hub connect: none",
            theme,
        )),
    }
    match summary.workspace_hub_bind.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub bind: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Workspace hub bind: none", theme)),
    }
    match summary.workspace_hub_upload.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub upload: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Workspace hub upload: none",
            theme,
        )),
    }
    match summary.workspace_hub_recover.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Workspace hub recover: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Workspace hub recover: none",
            theme,
        )),
    }
    match summary.graph_batch.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Graph batch: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Graph batch: none", theme)),
    }
    match summary.graph_query_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Graph query last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Graph query last: none", theme)),
    }
    match summary.graph_batch_first.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Graph batch first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Graph batch first: none", theme)),
    }
    match summary.persistent_graph.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Persistent graph: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Persistent graph: none", theme)),
    }
    match summary.cow_clone.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("COW clone: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("COW clone: none", theme)),
    }
    match summary.cow_clone_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("COW clone last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("COW clone last: none", theme)),
    }
    match summary.cow_fastpath.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("COW fastpath: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("COW fastpath: none", theme)),
    }
    match summary.browser_oidc.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Browser OIDC: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Browser OIDC: none", theme)),
    }
    match summary.browser_oidc_availability.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Browser OIDC availability: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Browser OIDC availability: none",
            theme,
        )),
    }
    match summary.browser_oidc_start.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Browser OIDC start: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Browser OIDC start: none", theme)),
    }
    match summary.browser_oidc_complete.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Browser OIDC complete: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Browser OIDC complete: none",
            theme,
        )),
    }
    match summary.mcp_oauth.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("MCP OAuth: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("MCP OAuth: none", theme)),
    }
    match summary.mcp_oauth_remote_availability.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("MCP OAuth remote availability: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "MCP OAuth remote availability: none",
            theme,
        )),
    }
    match summary.mcp_oauth_begin.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("MCP OAuth begin: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("MCP OAuth begin: none", theme)),
    }
    match summary.mcp_oauth_exchange.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("MCP OAuth exchange: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("MCP OAuth exchange: none", theme)),
    }
    match summary.mcp_oauth_open.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("MCP OAuth open: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("MCP OAuth open: none", theme)),
    }
    match summary.sleep_wake.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sleep/wake: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Sleep/wake: none", theme)),
    }
    match summary.sleep_wake_policy.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sleep/wake policy: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Sleep/wake policy: none", theme)),
    }
    match summary.sleep_wake_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sleep/wake last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Sleep/wake last: none", theme)),
    }
    match summary.sleep_wake_decision.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sleep/wake decision: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Sleep/wake decision: none", theme)),
    }
    match summary.sleep_wake_availability.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sleep/wake availability: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Sleep/wake availability: none",
            theme,
        )),
    }
    match summary.binary_update.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Binary update: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Binary update: none", theme)),
    }
    match summary.binary_update_policy.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Binary update policy: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Binary update policy: none",
            theme,
        )),
    }
    match summary.binary_update_check.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Binary update check: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Binary update check: none", theme)),
    }
    match summary.binary_version.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Binary version: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Binary version: none", theme)),
    }
    match summary.foreign_discover.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Foreign discover: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Foreign discover: none", theme)),
    }
    match summary.foreign_import_next.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Foreign import: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Foreign import: none", theme)),
    }
    match summary.foreign_import_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Foreign import last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Foreign import last: none", theme)),
    }
    match summary.jujutsu.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Jujutsu: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Jujutsu: none", theme)),
    }
    match summary.jujutsu_cli.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Jujutsu CLI: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Jujutsu CLI: none", theme)),
    }
    match summary.jujutsu_workspace.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Jujutsu workspace: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Jujutsu workspace: none", theme)),
    }
    match summary.jujutsu_last_command.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Jujutsu command last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Jujutsu command last: none",
            theme,
        )),
    }
    match summary.sandbox.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Sandbox: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Sandbox: none", theme)),
    }
    match summary.landlock.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("Landlock: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("Landlock: none", theme)),
    }
    match summary.os_sandbox_profiles.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("OS sandbox profiles: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("OS sandbox profiles: none", theme)),
    }
    match summary.os_sandbox_first_profile.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("OS sandbox first profile: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "OS sandbox first profile: none",
            theme,
        )),
    }
    match summary.sandbox_last_prepare.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Sandbox prepare last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Sandbox prepare last: none",
            theme,
        )),
    }
    match summary.acp.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("ACP: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("ACP: none", theme)),
    }
    match summary.acp_state.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("ACP state: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("ACP state: none", theme)),
    }
    match summary.acp_session.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("ACP session: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("ACP session: none", theme)),
    }
    match summary.acp_last_connect.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("ACP connect: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("ACP connect: none", theme)),
    }
    match summary.acp_last_bind.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(format!("ACP bind: {text}"), theme)),
        None => lines.push(status_dialog_plain_line("ACP bind: none", theme)),
    }
    match summary.edit_attribution.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Edit attribution: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Edit attribution: none", theme)),
    }
    match summary.edit_attribution_first.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Edit attribution first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Edit attribution first: none",
            theme,
        )),
    }
    match summary.edit_attribution_last.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Edit attribution last: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line(
            "Edit attribution last: none",
            theme,
        )),
    }
    match summary.plan_view.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plan view: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plan view: none", theme)),
    }
    match summary.plan_view_first.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plan view first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plan view first: none", theme)),
    }
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

fn append_status_dialog_plugins_section(
    lines: &mut Vec<Line<'static>>,
    summary: PluginDialogSummary,
    theme: &Theme,
) {
    if summary.installed == 0 {
        lines.push(status_dialog_plain_line("No Plugins", theme));
    } else {
        lines.push(status_dialog_plain_line(
            format!(
                "Plugins: {} installed ({} enabled, {} disabled)",
                summary.installed, summary.enabled, summary.disabled
            ),
            theme,
        ));
    }
    match summary.extension_descriptor.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Extension: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Extension: none", theme)),
    }
    match summary.last_install.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plugin install: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plugin install: none", theme)),
    }
    match summary.last_activate.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plugin activate: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plugin activate: none", theme)),
    }
    match summary.last_deactivate.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plugin deactivate: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plugin deactivate: none", theme)),
    }
    match summary.last_remove.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plugin remove: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plugin remove: none", theme)),
    }
    match summary.first_plugin.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Plugin first: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Plugin first: none", theme)),
    }
    match summary.discover.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Extension discover: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Extension discover: none", theme)),
    }
    match summary.last_load.as_deref() {
        Some(text) => lines.push(status_dialog_plain_line(
            format!("Extension load: {text}"),
            theme,
        )),
        None => lines.push(status_dialog_plain_line("Extension load: none", theme)),
    }
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
            .fg(theme.text.secondary)
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
    let dot = if row.enabled { "●" } else { "○" };
    let mut spans = vec![
        Span::styled(
            format!("{dot} "),
            Style::default().fg(dot_color).bg(surface),
        ),
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
fn status_dialog_edit_applied_envelope(
    seq: u64,
    path: &str,
    new_file_digest: &str,
) -> harness_core::event::EventEnvelopeV1 {
    use harness_core::event::{ActorKind, EventActor, EventEnvelopeV1, EventV1, SCHEMA_VERSION};
    EventEnvelopeV1 {
        schema_version: SCHEMA_VERSION,
        event_id: format!("evt-edit-{seq:04}"),
        seq,
        run_id: "run_status_dialog".into(),
        mono_ms: seq,
        ts: None,
        actor: EventActor::new(ActorKind::System, Some("status-dialog-test".to_string())),
        correlation_id: None,
        causation_id: None,
        stream_key: None,
        payload: EventV1::EditApplied(harness_core::event::EditAppliedEvent {
            edit_id: format!("edit_{seq}"),
            path: path.to_string(),
            new_file_digest: new_file_digest.to_string(),
            diff_rel_path: None,
            diff_digest: None,
        }),
    }
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_edit_attribution_counts_external_on_disk_drift() {
    // arrange
    // act
    // assert
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("notes.txt");
    std::fs::write(&path, b"agent-bytes").expect("write agent");
    let agent_digest = harness_core::edit_attribution::content_digest12(b"agent-bytes");
    std::fs::write(&path, b"human-bytes").expect("write external");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(dir.path().to_path_buf());
    app.ingest_event(status_dialog_edit_applied_envelope(
        1,
        "notes.txt",
        &agent_digest,
    ));

    let summary = status_dialog_edit_attribution_summary(&app);
    assert_eq!(summary.agent_tool, 0);
    assert_eq!(summary.external, 1);
    assert_eq!(summary.total, 1);

    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_edit_attribution_section(&mut lines, summary, &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Edit attribution: 0 agent-tool, 1 external"),
        "expected external drift line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_edit_attribution_keeps_matching_agent_tool() {
    // arrange
    // act
    // assert
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("kept.rs");
    std::fs::write(&path, b"stable").expect("write");
    let digest = harness_core::edit_attribution::content_digest12(b"stable");

    let mut app = AppState::new_live(None, false, None);
    app.set_file_mention_workspace_root_for_test(dir.path().to_path_buf());
    app.ingest_event(status_dialog_edit_applied_envelope(1, "kept.rs", &digest));

    let summary = status_dialog_edit_attribution_summary(&app);
    assert_eq!(summary.agent_tool, 1);
    assert_eq!(summary.external, 0);
    assert_eq!(summary.total, 1);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_edit_attribution_event_only_without_workspace() {
    // arrange
    // act
    // assert
    let digest = harness_core::edit_attribution::content_digest12(b"x");
    let mut app = AppState::new_live(None, false, None);
    app.ingest_event(status_dialog_edit_applied_envelope(1, "orphan.rs", &digest));

    let summary = status_dialog_edit_attribution_summary(&app);
    assert_eq!(summary.agent_tool, 1);
    assert_eq!(summary.external, 0);
    assert_eq!(summary.total, 1);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_fallback_and_none_demote() {
    // arrange
    // act
    // assert
    let mut app = AppState::new_live(None, false, None);
    app.set_status_banner(Some("provider fallback: model-a → model-b".to_string()));

    let summary = status_dialog_operator_summary(&app);
    assert_eq!(
        summary.provider_fallback.as_deref(),
        Some("provider fallback: model-a → model-b")
    );
    assert!(summary.crash_or_recovery.is_none());
    assert!(summary.demote_handle.is_none());
    assert!(!summary.settings_bound);
    assert_eq!(summary.settings_writable_paths, 6);
    assert!(summary.settings_total >= 38);

    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary, &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        rendered.contains("Fallback: provider fallback: model-a → model-b"),
        "expected fallback line: {rendered}"
    );
    assert!(
        rendered.contains("Crash/recovery: none"),
        "expected empty crash line: {rendered}"
    );
    assert!(
        rendered.contains("Demote focus: none"),
        "expected empty demote line: {rendered}"
    );
    assert!(
        rendered.contains("Settings: unbound, 6 write paths"),
        "expected unbound settings line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_crash_recovery_banner() {
    // arrange
    // act
    // assert
    let mut app = AppState::new_live(None, false, None);
    app.set_status_banner(Some(
        "Previous crash detected. Action: reopen session run-abc".to_string(),
    ));

    let summary = status_dialog_operator_summary(&app);
    assert!(summary.provider_fallback.is_none());
    assert_eq!(
        summary.crash_or_recovery.as_deref(),
        Some("Previous crash detected. Action: reopen session run-abc")
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_bound_settings_counts() {
    // arrange
    // act
    // assert
    // Given: live app with project config bound for settings write paths
    let dir = std::env::temp_dir().join(format!(
        "harness-tui-status-settings-{}-{}",
        "bound",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("workspace");
    let path = dir.join("harness.json");
    std::fs::write(&path, r#"{ "hashline_edit": true }"#).expect("write config");

    let mut app = AppState::new_live(None, false, None);
    app.bind_settings_project_config(&path, true, true, true, true, true, false);

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(summary.settings_bound);
    assert_eq!(summary.settings_writable_paths, 6);
    assert_eq!(summary.settings_editable, 6);
    assert!(
        rendered.contains("Settings: bound, 6/6 writable editable"),
        "expected bound settings line: {rendered}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_settings_registry_counts() {
    // arrange
    // act
    // assert
    // Given: live app with settings-registry composition summary bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .settings_registry
        .is_none());

    app.set_settings_registry_summary(Some(harness_core::config::summarize_settings_registry()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .settings_registry
            .as_deref()
            .is_some_and(|text| text.contains("settings registry:")),
        "expected settings registry one_line: {:?}",
        summary.settings_registry
    );
    assert!(
        rendered.contains("Settings registry: settings registry:"),
        "expected settings registry line: {rendered}"
    );
    assert!(
        rendered.contains("runtime=") && rendered.contains("tui="),
        "expected runtime/tui counts: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_dashboard() {
    // arrange
    // act
    // assert
    // Given: unseeded app — plan_view is always computable (may bind 1 probe)
    let app = AppState::new_live(None, false, None);
    let summary = status_dialog_operator_summary(&app);
    let (bound, total) = summary.bound_probe_counts();
    assert!(
        bound <= 2,
        "unseeded app should bind few optional probes; bound={bound}"
    );
    assert!(
        total > 50,
        "dashboard tracks a large probe surface; total={total}"
    );
    assert!(
        summary
            .dashboard_one_line()
            .starts_with(&format!("operator dashboard: {bound} bound of ")),
        "line={}",
        summary.dashboard_one_line()
    );
    assert!(
        summary.plan_view.is_some(),
        "plan_view summary is always computable"
    );

    // When: bind a few additional operator surfaces
    let mut app = AppState::new_live(None, false, None);
    app.set_auto_fallback_last_banner(Some("provider fallback: a → b".to_string()));
    app.set_jujutsu_last_command(Some(
        harness_core::jujutsu::JujutsuCommandOutcome::Unavailable {
            command: "jj --version".to_string(),
            reason: "missing".to_string(),
        },
    ));
    app.set_graph_query_batch_first_line(Some(
        "graph query unavailable: symbol_def `(probe)` (no backend)".to_string(),
    ));
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    // Then
    let (bound, total) = summary.bound_probe_counts();
    assert!(
        bound >= 3,
        "expected at least 3 bound probes; bound={bound}"
    );
    assert!(
        summary
            .dashboard_one_line()
            .contains(&format!("{bound} bound of {total}")),
        "line={}",
        summary.dashboard_one_line()
    );
    assert!(
        rendered.contains("operator dashboard:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_dashboard_after_seed() {
    // arrange
    // act
    // assert
    // Given: live app with full operator host probe seed
    let root = std::env::temp_dir().join(format!(
        "harness-tui-status-dashboard-seed-{}-{}",
        "bound",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("workspace");
    let mut app = AppState::new_live(None, false, None);
    app.seed_operator_host_probes(Some(root.as_path()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let (bound, total) = summary.bound_probe_counts();

    // Then: seed binds a large operator probe surface on the dashboard
    assert!(
        total > 50,
        "dashboard tracks a large probe surface; total={total}"
    );
    assert!(
        bound >= 40,
        "expected full seed to bind many probes; bound={bound} total={total}"
    );
    assert!(
        summary
            .dashboard_one_line()
            .contains(&format!("{bound} bound of {total}")),
        "line={}",
        summary.dashboard_one_line()
    );
    assert!(
        rendered.contains("operator dashboard:"),
        "rendered={rendered}"
    );
    assert!(summary.settings_bound || summary.settings_registry.is_some());
    assert!(summary.plan_view.is_some());
    assert!(summary.binary_update.is_some() || summary.binary_version.is_some());

    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_crash_scan_counts() {
    // arrange
    // act
    // assert
    // Given: live app with multi-run crash scan summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).crash_scan.is_none());

    app.set_crash_recovery_scan_summary(Some(
        harness_core::crash_recovery::CrashRecoveryScanSummary {
            scanned: 3,
            previous_crash: 1,
            clean: 2,
            stale_writer_lock: 1,
            recovery_marker: 1,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .crash_scan
            .as_deref()
            .is_some_and(|text| text.contains("crash scan:")),
        "expected crash scan one_line: {:?}",
        summary.crash_scan
    );
    assert!(
        rendered.contains("Crash scan: crash scan:"),
        "expected crash scan line: {rendered}"
    );
    assert!(
        rendered.contains("previous-crash"),
        "expected previous-crash count in crash scan line: {rendered}"
    );
}

#[cfg(test)]
#[test]

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_edit_attribution() {
    // arrange
    // act
    // assert
    // Given: bound edit attribution summary
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .edit_attribution
        .is_none());
    app.set_edit_attribution_summary(Some(
        harness_core::edit_attribution::EditAttributionSummary {
            agent_tool: 2,
            external: 1,
            drift: 0,
            total: 3,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary
        .edit_attribution
        .as_deref()
        .expect("edit attribution line");
    assert!(line.contains("2 agent-tool"), "line={line}");
    assert!(line.contains("1 external"), "line={line}");
    assert!(
        rendered.contains("Edit attribution:"),
        "rendered={rendered}"
    );
}

#[allow(
    clippy::expect_used,
    clippy::field_reassign_with_default,
    reason = "test setup: fields set after Default ensures precondition for subsequent expect"
)]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_crash_recovery_next() {
    // Given: first previous-crash report bound with reopen action
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .crash_recovery_next
        .is_none());
    let mut report = harness_core::crash_recovery::PreviousCrashReport::default();
    report.run_dir = std::path::PathBuf::from("/tmp/sessions/run-crash-1");
    report.previous_crash_detected = true;
    report.stale_writer_lock = true;
    report.events_log_present = true;
    report.recovery_action = Some(harness_core::crash_recovery::CrashRecoveryAction::ReopenSession);
    report.recovery_message = Some("stale writer lock".to_string());
    app.set_crash_recovery_first_report(Some(report));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then: operator next-step includes previous-crash + reopen hint
    let next = summary
        .crash_recovery_next
        .as_deref()
        .expect("crash recovery next");
    assert!(next.contains("previous-crash"), "next={next}");
    assert!(
        next.contains("reopen") || next.contains("sessions reopen"),
        "next={next}"
    );
    assert!(rendered.contains("Crash recovery:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_team_registry_counts() {
    // Given: live app with team registry summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).teams.is_none());

    app.set_team_registry_summary(Some(harness_core::team_registry::TeamRegistrySummary {
        teams: 2,
        active: 1,
        cancelled: 1,
        members: 3,
        mailbox_messages: 4,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .teams
            .as_deref()
            .is_some_and(|text| text.contains("teams:")),
        "expected teams one_line: {:?}",
        summary.teams
    );
    assert!(
        rendered.contains("Teams: teams:"),
        "expected teams line: {rendered}"
    );
    assert!(
        rendered.contains("active") && rendered.contains("mailbox"),
        "expected active/mailbox counts in teams line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_edit_attribution_first_last() {
    // arrange
    // act
    // assert
    // Given: first/last edit attribution lines bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .edit_attribution_first
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .edit_attribution_last
        .is_none());

    app.set_edit_attribution_first_line(Some(
        "edit attribution: `src/a.rs` source=agent_tool".to_string(),
    ));
    app.set_edit_attribution_last_line(Some(
        "edit attribution: `src/b.rs` source=external".to_string(),
    ));
    app.set_edit_attribution_summary(Some(
        harness_core::edit_attribution::EditAttributionSummary {
            agent_tool: 1,
            external: 1,
            drift: 0,
            total: 2,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let first = summary
        .edit_attribution_first
        .as_deref()
        .expect("edit attribution first");
    let last = summary
        .edit_attribution_last
        .as_deref()
        .expect("edit attribution last");
    assert!(
        first.contains("agent_tool") || first.contains("edit attribution"),
        "first={first}"
    );
    assert!(
        last.contains("external") || last.contains("edit attribution"),
        "last={last}"
    );
    assert!(
        rendered.contains("Edit attribution first:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Edit attribution last:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_plan_view() {
    // arrange
    // act
    // assert
    // Given: live app (plan view summary always computable; first may be none)
    let app = AppState::new_live(None, false, None);

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let plan = summary.plan_view.as_deref().expect("plan view summary");
    assert!(
        plan.contains("plan view:") || plan.contains("total"),
        "plan={plan}"
    );
    // Empty workspace: first plan may be none (honest).
    assert!(rendered.contains("Plan view:"), "rendered={rendered}");
    assert!(rendered.contains("Plan view first:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_crash_recovery_action() {
    // arrange
    // act
    // assert
    // Given: crash recovery resolved action + first report one-line bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .crash_recovery_action
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .crash_recovery_report
        .is_none());

    app.set_crash_recovery_resolved_action(Some(
        harness_core::crash_recovery::resolve_crash_recovery_action(false),
    ));
    let report = harness_core::crash_recovery::PreviousCrashReport {
        run_dir: std::path::PathBuf::from("/tmp/run_crashed"),
        previous_crash_detected: true,
        stale_writer_lock: true,
        events_log_present: true,
        recovery_action: Some(harness_core::crash_recovery::CrashRecoveryAction::ReopenSession),
        ..Default::default()
    };
    app.set_crash_recovery_first_report_line(Some(report.one_line()));
    app.set_crash_recovery_first_report(Some(report));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let action = summary
        .crash_recovery_action
        .as_deref()
        .expect("crash recovery action");
    assert!(
        action.contains("reopen_session") || action.contains("reopen"),
        "action={action}"
    );
    let report_line = summary
        .crash_recovery_report
        .as_deref()
        .expect("crash recovery report");
    assert!(
        report_line.contains("previous-crash") || report_line.contains("run_crashed"),
        "report={report_line}"
    );
    assert!(
        rendered.contains("Crash recovery action:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Crash recovery report:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_team_create() {
    // arrange
    // act
    // assert
    // Given: team last-create + first team bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .team_last_create
        .is_none());
    assert!(status_dialog_operator_summary(&app).team_first.is_none());

    let mut registry = harness_core::team_registry::TeamRegistry::new();
    let outcome = harness_core::team_registry::create_team_outcome(&mut registry, "alpha");
    app.set_team_last_create(Some(outcome));
    if let Some(first) = registry.list_teams().first() {
        app.set_team_first_line(Some(first.one_line()));
    }
    app.set_team_registry_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let create = summary.team_last_create.as_deref().expect("team create");
    assert!(
        create.contains("ok") || create.contains("alpha"),
        "create={create}"
    );
    let first = summary.team_first.as_deref().expect("team first");
    assert!(
        first.contains("alpha") || first.contains("active"),
        "first={first}"
    );
    assert!(rendered.contains("Team create:"), "rendered={rendered}");
    assert!(rendered.contains("Team first:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_team_send() {
    // arrange
    // act
    // assert
    // Given: team last-send + last mailbox message bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .team_last_send
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .team_last_message
        .is_none());

    let mut registry = harness_core::team_registry::TeamRegistry::new();
    let created = registry.create_team("alpha").expect("create");
    registry
        .add_member(&created.team_id, "agent-a", "lead")
        .expect("add member");
    let send = harness_core::team_registry::send_team_message_outcome(
        &mut registry,
        &created.team_id,
        "agent-a",
        None,
        "hello team",
    );
    app.set_team_last_send(Some(send));
    if let Ok(msgs) = registry.peek_inbox(&created.team_id, "agent-a") {
        if let Some(last) = msgs.last() {
            app.set_team_last_message_line(Some(last.one_line()));
        }
    }
    app.set_team_registry_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let send = summary.team_last_send.as_deref().expect("team send");
    assert!(send.contains("ok") || send.contains("msg_"), "send={send}");
    let msg = summary
        .team_last_message
        .as_deref()
        .expect("team mailbox last");
    assert!(
        msg.contains("hello") || msg.contains("agent-a"),
        "msg={msg}"
    );
    assert!(rendered.contains("Team send:"), "rendered={rendered}");
    assert!(
        rendered.contains("Team mailbox last:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_team_add_cancel() {
    // arrange
    // act
    // assert
    // Given: team last-add-member + last-cancel bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .team_add_member
        .is_none());
    assert!(status_dialog_operator_summary(&app).team_cancel.is_none());

    let mut registry = harness_core::team_registry::TeamRegistry::new();
    let created = registry.create_team("alpha").expect("create");
    let add = harness_core::team_registry::add_team_member_outcome(
        &mut registry,
        &created.team_id,
        "agent-a",
        "lead",
    );
    app.set_team_last_add_member(Some(add));
    let cancel = harness_core::team_registry::cancel_team_outcome(&mut registry, &created.team_id);
    app.set_team_last_cancel(Some(cancel));
    app.set_team_registry_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let add = summary.team_add_member.as_deref().expect("team add-member");
    assert!(add.contains("ok") || add.contains("agent-a"), "add={add}");
    let cancel = summary.team_cancel.as_deref().expect("team cancel");
    assert!(
        cancel.contains("ok") || cancel.contains("alpha"),
        "cancel={cancel}"
    );
    assert!(rendered.contains("Team add-member:"), "rendered={rendered}");
    assert!(rendered.contains("Team cancel:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_acp_connection() {
    // arrange
    // act
    // assert
    // Given: live app with ACP connection summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).acp.is_none());

    app.set_acp_connection_summary(Some(harness_core::integrations::AcpConnectionSummary {
        state: "connected".to_string(),
        session_id: Some("sess-1".to_string()),
        agent_name: Some("demo-agent".to_string()),
        bound: true,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .acp
            .as_deref()
            .is_some_and(|text| text.contains("ACP: state=connected")),
        "expected ACP one_line: {:?}",
        summary.acp
    );
    assert!(
        rendered.contains("ACP: ACP: state=connected"),
        "expected ACP line: {rendered}"
    );
    assert!(
        rendered.contains("session=`sess-1`") && rendered.contains("agent=`demo-agent`"),
        "expected session/agent honesty in ACP line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_acp_session() {
    // arrange
    // act
    // assert
    // Given: ACP state + session bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).acp_state.is_none());
    assert!(status_dialog_operator_summary(&app).acp_session.is_none());
    app.set_acp_connection_state(Some(
        harness_core::integrations::AcpConnectionState::Connected,
    ));
    app.set_acp_session_info(Some(harness_core::integrations::AcpSessionInfo {
        session_id: "acp-session-1".to_string(),
        agent_name: "demo-agent".to_string(),
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let state = summary.acp_state.as_deref().expect("acp state");
    assert!(state.contains("connected"), "state={state}");
    let session = summary.acp_session.as_deref().expect("acp session");
    assert!(session.contains("acp-session-1"), "session={session}");
    assert!(session.contains("demo-agent"), "session={session}");
    assert!(rendered.contains("ACP state:"), "rendered={rendered}");
    assert!(rendered.contains("ACP session:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_sandbox_fs_plan() {
    // Given: live app with sandbox FS-plan summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).sandbox.is_none());

    app.set_sandbox_fs_plan_summary(Some(harness_core::sandbox::SandboxFsPlanSummary {
        policy: harness_core::sandbox::SandboxPolicy::WorkspaceWrite,
        read_root_count: 2,
        write_root_count: 1,
        read_roots: vec!["/ws".to_string(), "/tmp".to_string()],
        write_roots: vec!["/ws".to_string()],
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .sandbox
            .as_deref()
            .is_some_and(|text| text.contains("policy=workspace_write")),
        "expected sandbox one_line: {:?}",
        summary.sandbox
    );
    assert!(
        rendered.contains("Sandbox: policy=workspace_write"),
        "expected sandbox line: {rendered}"
    );
    assert!(
        rendered.contains("read_roots=2") && rendered.contains("write_roots=1"),
        "expected root counts in sandbox line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_acp_connect_bind() {
    // arrange
    // act
    // assert
    // Given: ACP last-connect + last-bind fail-closed outcomes bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .acp_last_connect
        .is_none());
    assert!(status_dialog_operator_summary(&app).acp_last_bind.is_none());

    let mut transport = harness_core::integrations::MockAcpTransport::new();
    transport.fail_connect = true;
    transport.fail_connect_reason = "offline MVP".to_string();
    let mut acp = harness_core::integrations::AcpConnection::new(transport);
    app.set_acp_last_connect(Some(harness_core::integrations::connect_acp_outcome(
        &mut acp,
    )));
    app.set_acp_last_bind(Some(harness_core::integrations::bind_acp_session_outcome(
        &mut acp, "(probe)",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let connect = summary.acp_last_connect.as_deref().expect("acp connect");
    let bind = summary.acp_last_bind.as_deref().expect("acp bind");
    assert!(
        connect.contains("failed") || connect.contains("ACP connect"),
        "connect={connect}"
    );
    assert!(
        bind.contains("failed") || bind.contains("ACP bind"),
        "bind={bind}"
    );
    assert!(rendered.contains("ACP connect:"), "rendered={rendered}");
    assert!(rendered.contains("ACP bind:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_jujutsu_probe() {
    // arrange
    // act
    // assert
    // Given: live app with jujutsu probe bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).jujutsu.is_none());

    app.set_jujutsu_probe(Some(harness_core::jujutsu::JujutsuProbe {
        cli: harness_core::jujutsu::JujutsuAvailability::Unavailable {
            reason: "jujutsu CLI `jj` not found on PATH".to_string(),
        },
        workspace: harness_core::jujutsu::JujutsuWorkspaceStatus::NotARepo {
            workspace_root: std::path::PathBuf::from("/tmp/ws"),
            reason: "no .jj marker".to_string(),
        },
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .jujutsu
            .as_deref()
            .is_some_and(|text| text.contains("cli=unavailable") || text.contains("ready=false")),
        "expected jujutsu probe one_line: {:?}",
        summary.jujutsu
    );
    assert!(
        rendered.contains("Jujutsu:"),
        "expected jujutsu line: {rendered}"
    );
    assert!(
        rendered.contains("ready=false"),
        "expected honest ready=false in jujutsu line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_jujutsu_components() {
    // arrange
    // act
    // assert
    // Given: jujutsu CLI + workspace components bound separately
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).jujutsu_cli.is_none());
    assert!(status_dialog_operator_summary(&app)
        .jujutsu_workspace
        .is_none());

    app.set_jujutsu_cli(Some(
        harness_core::jujutsu::JujutsuAvailability::Unavailable {
            reason: "jujutsu CLI `jj` not found on PATH".to_string(),
        },
    ));
    app.set_jujutsu_workspace(Some(
        harness_core::jujutsu::JujutsuWorkspaceStatus::NotARepo {
            workspace_root: std::path::PathBuf::from("/tmp/ws"),
            reason: "no .jj directory".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let cli = summary.jujutsu_cli.as_deref().expect("jujutsu cli");
    assert!(cli.contains("unavailable"), "cli={cli}");
    let workspace = summary
        .jujutsu_workspace
        .as_deref()
        .expect("jujutsu workspace");
    assert!(
        workspace.contains("not_a_repo") || workspace.contains("no .jj"),
        "workspace={workspace}"
    );
    assert!(rendered.contains("Jujutsu CLI:"), "rendered={rendered}");
    assert!(
        rendered.contains("Jujutsu workspace:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_jujutsu_last_command() {
    // arrange
    // act
    // assert
    // Given: last jujutsu command outcome bound (unavailable)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .jujutsu_last_command
        .is_none());
    app.set_jujutsu_last_command(Some(
        harness_core::jujutsu::JujutsuCommandOutcome::Unavailable {
            command: "jj --version".to_string(),
            reason: "jujutsu CLI `jj` not found on PATH; jujutsu workflows unavailable".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    // Then
    let line = summary
        .jujutsu_last_command
        .as_deref()
        .expect("jujutsu last command line");
    assert!(
        line.contains("jujutsu command:") && line.contains("--version"),
        "line={line}"
    );
    assert!(
        rendered.contains("Jujutsu command last:"),
        "rendered={rendered}"
    );
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_foreign_discover_counts() {
    // Given: live app with foreign-discover summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .foreign_discover
        .is_none());

    app.set_foreign_discover_summary(Some(
        harness_core::foreign_session::ForeignDiscoverSummary {
            discoverable: 3,
            importable: 1,
            discoverable_not_importable: 2,
            corrupt: 0,
            rejected: 1,
            total: 4,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .foreign_discover
            .as_deref()
            .is_some_and(|text| text.contains("foreign discover:")),
        "expected foreign discover one_line: {:?}",
        summary.foreign_discover
    );
    assert!(
        rendered.contains("Foreign discover: foreign discover:"),
        "expected foreign discover line: {rendered}"
    );
    assert!(
        rendered.contains("1 importable") && rendered.contains("2 not yet"),
        "expected importable honesty in foreign discover line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_foreign_import_next() {
    // arrange
    // act
    // assert
    // Given: first importable foreign candidate bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .foreign_import_next
        .is_none());
    app.set_foreign_import_first_candidate(Some(
        harness_core::foreign_session::ForeignSessionCandidate::Discoverable {
            kind: harness_core::foreign_session::ForeignAgentKind::Unknown,
            path: std::path::PathBuf::from("/tmp/foreign/run-importable"),
            marker: "events.jsonl".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let next = summary
        .foreign_import_next
        .as_deref()
        .expect("foreign import next");
    assert!(next.contains("importable"), "next={next}");
    assert!(next.contains("events.jsonl"), "next={next}");
    assert!(
        next.contains("run-importable") || next.contains("import-foreign"),
        "next={next}"
    );
    assert!(rendered.contains("Foreign import:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_binary_update_counts() {
    // Given: live app with binary-update summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).binary_update.is_none());

    app.set_binary_update_summary(Some(harness_core::binary_update::BinaryUpdateSummary {
        checks_unavailable: 2,
        checks_up_to_date: 0,
        total: 2,
        update_available: false,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .binary_update
            .as_deref()
            .is_some_and(|text| text.contains("binary update:")),
        "expected binary update one_line: {:?}",
        summary.binary_update
    );
    assert!(
        rendered.contains("Binary update: binary update:"),
        "expected binary update line: {rendered}"
    );
    assert!(
        rendered.contains("update_available=false"),
        "expected honest update_available=false in binary update line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_foreign_import_last() {
    // arrange
    // act
    // assert
    // Given: foreign import last-outcome bound (fail-closed missing source)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .foreign_import_last
        .is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-foreign-import-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let probe_src = root.join("(probe-foreign)");
    let probe_dest = root.join("(probe-import-dest)");
    app.set_foreign_import_last_outcome(Some(
        harness_core::foreign_session::import_foreign_session_outcome(&probe_src, &probe_dest),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let text = summary
        .foreign_import_last
        .as_deref()
        .expect("foreign import last");
    assert!(
        text.contains("failed") || text.contains("ok"),
        "text={text}"
    );
    assert!(
        rendered.contains("Foreign import last:"),
        "rendered={rendered}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_binary_update_policy() {
    // arrange
    // act
    // assert
    // Given: bound offline policy + unavailable check
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .binary_update_policy
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .binary_update_check
        .is_none());

    let policy = harness_core::binary_update::BinaryUpdatePolicy::new()
        .with_channel("offline")
        .with_min_version("0.1.0");
    let check = harness_core::binary_update::check_for_update_with_policy("0.1.0", policy.clone());
    app.set_binary_update_policy(Some(policy));
    app.set_binary_update_check(Some(check));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let policy_line = summary
        .binary_update_policy
        .as_deref()
        .expect("policy line");
    assert!(
        policy_line.contains("channel=offline"),
        "policy={policy_line}"
    );
    assert!(
        policy_line.contains("min_version=0.1.0"),
        "policy={policy_line}"
    );
    let check_line = summary.binary_update_check.as_deref().expect("check line");
    assert!(check_line.contains("unavailable"), "check={check_line}");
    assert!(
        check_line.contains("channel=offline") || check_line.contains("offline"),
        "check={check_line}"
    );
    assert!(
        rendered.contains("Binary update policy:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Binary update check:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_sleep_wake_observations() {
    // arrange
    // act
    // assert
    // Given: live app with sleep/wake observation summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).sleep_wake.is_none());

    app.set_sleep_wake_observation_summary(Some(
        harness_core::sleep_wake_auth::SleepWakeObservationSummary {
            recorded: 2,
            recorded_noop: 0,
            total: 2,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .sleep_wake
            .as_deref()
            .is_some_and(|text| text.contains("sleep/wake observations:")),
        "expected sleep/wake one_line: {:?}",
        summary.sleep_wake
    );
    assert!(
        rendered.contains("Sleep/wake: sleep/wake observations:"),
        "expected sleep/wake line: {rendered}"
    );
    assert!(
        rendered.contains("2 recorded"),
        "expected recorded count in sleep/wake line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_binary_version() {
    // arrange
    // act
    // assert
    // Given: binary version info bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .binary_version
        .is_none());

    let info = harness_core::binary_update::current_binary_version();
    app.set_binary_version_info(Some(info.clone()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let version = summary.binary_version.as_deref().expect("binary version");
    assert!(
        version.contains("harness") || version.contains("binary:"),
        "version={version}"
    );
    assert!(
        version.contains(&info.version) || version.contains("0."),
        "version={version} expected {}",
        info.version
    );
    assert!(rendered.contains("Binary version:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_sleep_wake_policy() {
    // arrange
    // act
    // assert
    // Given: sleep/wake policy + last observation bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .sleep_wake_policy
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .sleep_wake_last
        .is_none());

    app.set_sleep_wake_credential_policy(Some(
        harness_core::sleep_wake_auth::evaluate_sleep_wake_credential_refresh(),
    ));
    app.set_sleep_wake_last_observation(Some(
        harness_core::sleep_wake_auth::observe_sleep_wake_host_event(
            harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake,
        ),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let policy = summary
        .sleep_wake_policy
        .as_deref()
        .expect("sleep/wake policy");
    assert!(policy.contains("active (strategy=hook)"), "policy={policy}");
    let last = summary.sleep_wake_last.as_deref().expect("sleep/wake last");
    assert!(last.contains("wake"), "last={last}");
    assert!(
        last.contains("recorded") || last.contains("active"),
        "last={last}"
    );
    assert!(
        rendered.contains("Sleep/wake policy:"),
        "rendered={rendered}"
    );
    assert!(rendered.contains("Sleep/wake last:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_outcomes() {
    // Given: live app with MCP OAuth outcome summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).mcp_oauth.is_none());

    app.set_mcp_oauth_outcome_summary(Some(harness_core::mcp_oauth::McpOauthOutcomeSummary {
        begin_unavailable: 1,
        exchange_unavailable: 1,
        open_unavailable: 1,
        total: 3,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .mcp_oauth
            .as_deref()
            .is_some_and(|text| text.contains("MCP OAuth outcomes:")),
        "expected MCP OAuth one_line: {:?}",
        summary.mcp_oauth
    );
    assert!(
        rendered.contains("MCP OAuth: MCP OAuth outcomes:"),
        "expected MCP OAuth line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable"),
        "expected unavailable honesty in MCP OAuth line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_sleep_wake_availability() {
    // arrange
    // act
    // assert
    // Given: sleep/wake availability + last wake observation bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .sleep_wake_availability
        .is_none());

    app.set_sleep_wake_availability(Some(
        harness_core::sleep_wake_auth::sleep_wake_credential_refresh_availability(),
    ));
    let wake = harness_core::sleep_wake_auth::observe_sleep_wake_host_event(
        harness_core::sleep_wake_auth::SleepWakeHostEvent::Wake,
    );
    app.set_sleep_wake_last_observation(Some(wake.clone()));
    app.set_sleep_wake_observation_summary(Some(
        harness_core::sleep_wake_auth::summarize_sleep_wake_observations(&[wake]),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let availability = summary
        .sleep_wake_availability
        .as_deref()
        .expect("sleep/wake availability");
    assert!(
        availability.contains("active"),
        "availability={availability}"
    );
    let last = summary.sleep_wake_last.as_deref().expect("sleep/wake last");
    assert!(
        last.contains("wake") || last.contains("recorded"),
        "last={last}"
    );
    assert!(
        rendered.contains("Sleep/wake availability:"),
        "rendered={rendered}"
    );
    assert!(rendered.contains("Sleep/wake last:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_remote_availability() {
    // arrange
    // act
    // assert
    // Given: MCP OAuth remote availability + last begin bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .mcp_oauth_remote_availability
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .mcp_oauth_begin
        .is_none());

    app.set_mcp_oauth_remote_availability(Some(
        harness_core::mcp_oauth::evaluate_mcp_oauth_remote_transports(),
    ));
    app.set_mcp_oauth_last_begin(Some(harness_core::mcp_oauth::begin_mcp_oauth_flow(
        "docs-server",
        "https://auth.example/oauth",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let availability = summary
        .mcp_oauth_remote_availability
        .as_deref()
        .expect("MCP OAuth remote availability");
    assert!(
        availability.contains("unavailable"),
        "availability={availability}"
    );
    let begin = summary.mcp_oauth_begin.as_deref().expect("MCP OAuth begin");
    assert!(begin.contains("begun"), "begin={begin}");
    assert!(
        begin.contains("docs-server") || begin.contains("auth.example"),
        "begin={begin}"
    );
    assert!(
        rendered.contains("MCP OAuth remote availability:"),
        "rendered={rendered}"
    );
    assert!(rendered.contains("MCP OAuth begin:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_browser_oidc_outcomes() {
    // Given: live app with browser-OIDC outcome summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).browser_oidc.is_none());

    app.set_browser_oidc_outcome_summary(Some(
        harness_core::browser_oidc::BrowserOidcOutcomeSummary {
            start_unavailable: 1,
            complete_unavailable: 1,
            total: 2,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .browser_oidc
            .as_deref()
            .is_some_and(|text| text.contains("browser OIDC outcomes:")),
        "expected browser OIDC one_line: {:?}",
        summary.browser_oidc
    );
    assert!(
        rendered.contains("Browser OIDC: browser OIDC outcomes:"),
        "expected browser OIDC line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable"),
        "expected unavailable honesty in browser OIDC line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_mcp_oauth_exchange_open() {
    // arrange
    // act
    // assert
    // Given: MCP OAuth last-exchange + last-open bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .mcp_oauth_exchange
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .mcp_oauth_open
        .is_none());

    app.set_mcp_oauth_last_exchange(Some(harness_core::mcp_oauth::exchange_mcp_oauth_token(
        "docs-server",
        "abcd1234secret",
    )));
    app.set_mcp_oauth_last_open(Some(harness_core::mcp_oauth::open_mcp_remote_transport(
        "docs-server",
        "https://mcp.example/sse",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let exchange = summary
        .mcp_oauth_exchange
        .as_deref()
        .expect("MCP OAuth exchange");
    assert!(exchange.contains("exchanged"), "exchange={exchange}");
    assert!(
        exchange.contains("docs-server") || exchange.contains("abcd…"),
        "exchange={exchange}"
    );
    assert!(
        !exchange.contains("secret"),
        "must not leak secret: {exchange}"
    );
    let open = summary.mcp_oauth_open.as_deref().expect("MCP OAuth open");
    assert!(open.contains("unavailable"), "open={open}");
    assert!(
        open.contains("docs-server") || open.contains("mcp.example"),
        "open={open}"
    );
    assert!(
        rendered.contains("MCP OAuth exchange:"),
        "rendered={rendered}"
    );
    assert!(rendered.contains("MCP OAuth open:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_browser_oidc_availability() {
    // arrange
    // act
    // assert
    // Given: browser OIDC availability + last start bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .browser_oidc_availability
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .browser_oidc_start
        .is_none());

    app.set_browser_oidc_availability(Some(
        harness_core::browser_oidc::evaluate_browser_oidc_availability(),
    ));
    app.set_browser_oidc_last_start(Some(harness_core::browser_oidc::start_browser_oidc_flow(
        "https://issuer.example",
        "client-abc",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let availability = summary
        .browser_oidc_availability
        .as_deref()
        .expect("browser OIDC availability");
    assert!(
        availability.contains("available"),
        "availability={availability}"
    );
    let start = summary
        .browser_oidc_start
        .as_deref()
        .expect("browser OIDC start");
    assert!(start.contains("started"), "start={start}");
    assert!(
        start.contains("issuer.example") || start.contains("client-abc"),
        "start={start}"
    );
    assert!(
        rendered.contains("Browser OIDC availability:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Browser OIDC start:"),
        "rendered={rendered}"
    );
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cow_clone_outcomes() {
    // Given: live app with COW-clone outcome summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).cow_clone.is_none());

    app.set_cow_clone_outcome_summary(Some(harness_core::cow_worktree::CowCloneOutcomeSummary {
        cloned: 1,
        unavailable: 2,
        total: 3,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .cow_clone
            .as_deref()
            .is_some_and(|text| text.contains("COW clone outcomes:")),
        "expected COW clone one_line: {:?}",
        summary.cow_clone
    );
    assert!(
        rendered.contains("COW clone: COW clone outcomes:"),
        "expected COW clone line: {rendered}"
    );
    assert!(
        rendered.contains("1 cloned") && rendered.contains("2 unavailable"),
        "expected cloned/unavailable honesty in COW clone line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_browser_oidc_complete() {
    // arrange
    // act
    // assert
    // Given: browser OIDC last-complete bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .browser_oidc_complete
        .is_none());

    app.set_browser_oidc_last_complete(Some(
        harness_core::browser_oidc::BrowserOidcCompleteResult::Completed {
            token_type: "Bearer".to_string(),
            access_token_redacted: "abcd…".to_string(),
            has_id_token: true,
            has_refresh_token: false,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let text = summary
        .browser_oidc_complete
        .as_deref()
        .expect("browser OIDC complete");
    assert!(text.contains("completed"), "text={text}");
    assert!(
        text.contains("abcd…") || text.contains("token="),
        "text={text}"
    );
    assert!(!text.contains("secret"), "must not leak secret: {text}");
    assert!(
        rendered.contains("Browser OIDC complete:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cow_clone_last() {
    // arrange
    // act
    // assert
    // Given: last COW clone result bound as unavailable diagnostic
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .cow_clone_last
        .is_none());
    app.set_cow_clone_last_result(Some(
        harness_core::cow_worktree::CowCloneResult::Unavailable {
            reason: "probe only".to_string(),
            platform: "linux".to_string(),
            src: "/tmp/a".to_string(),
            dst: "/tmp/b".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary.cow_clone_last.as_deref().expect("cow clone last");
    assert!(line.contains("unavailable"), "line={line}");
    assert!(
        line.contains("probe only") || line.contains("/tmp/a"),
        "line={line}"
    );
    assert!(rendered.contains("COW clone last:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_graph_query_batch() {
    // Given: live app with graph-query batch summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).graph_batch.is_none());

    app.set_graph_query_batch_summary(Some(harness_core::code_graph::GraphQueryBatchSummary {
        total: 3,
        unavailable: 3,
        hits: 0,
        hit_results: 0,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .graph_batch
            .as_deref()
            .is_some_and(|text| text.contains("graph batch:")),
        "expected graph batch one_line: {:?}",
        summary.graph_batch
    );
    assert!(
        rendered.contains("Graph batch: graph batch:"),
        "expected graph batch line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable") || rendered.contains("hit"),
        "expected graph batch counts in line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_graph_query_last() {
    // arrange
    // act
    // assert
    // Given: last graph query result bound as structured unavailable
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .graph_query_last
        .is_none());
    app.set_graph_query_last_result(Some(
        harness_core::code_graph::GraphQueryResult::Unavailable {
            reason: "no first-party persistent graph".to_string(),
            symbol: "Coordinator".to_string(),
            kind: harness_core::code_graph::GraphQueryKind::SymbolDef,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary
        .graph_query_last
        .as_deref()
        .expect("graph query last");
    assert!(line.contains("unavailable"), "line={line}");
    assert!(
        line.contains("Coordinator") || line.contains("symbol_def"),
        "line={line}"
    );
    assert!(
        rendered.contains("Graph query last:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_graph_batch_first() {
    // arrange
    // act
    // assert
    // Given: first result one_line from multi-kind batch bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .graph_batch_first
        .is_none());
    app.set_graph_query_batch_first_line(Some(
        "graph query unavailable: symbol_def `(probe)` (no first-party persistent incremental codebase graph; external indexes are not claimed as harness product surfaces)".to_string(),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    // Then
    let line = summary
        .graph_batch_first
        .as_deref()
        .expect("graph batch first line");
    assert!(
        line.contains("symbol_def") && line.contains("(probe)"),
        "line={line}"
    );
    assert!(
        rendered.contains("Graph batch first:"),
        "rendered={rendered}"
    );
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_workspace_hub_outcomes() {
    // Given: live app with workspace-hub outcome summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).workspace_hub.is_none());

    app.set_workspace_hub_outcome_summary(Some(
        harness_core::workspace_hub::WorkspaceHubOutcomeSummary {
            connect_unavailable: 1,
            bind_unavailable: 1,
            upload_unavailable: 0,
            recover_unavailable: 1,
            total: 3,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .workspace_hub
            .as_deref()
            .is_some_and(|text| text.contains("workspace hub outcomes:")),
        "expected workspace hub one_line: {:?}",
        summary.workspace_hub
    );
    assert!(
        rendered.contains("Workspace hub: workspace hub outcomes:"),
        "expected workspace hub line: {rendered}"
    );
    assert!(
        rendered.contains("connect=1") && rendered.contains("unavailable"),
        "expected unavailable honesty in workspace hub line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_workspace_hub_availability() {
    // arrange
    // act
    // assert
    // Given: workspace hub availability + last connect bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .workspace_hub_availability
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .workspace_hub_connect
        .is_none());

    app.set_workspace_hub_availability(Some(harness_core::workspace_hub::evaluate_workspace_hub()));
    app.set_workspace_hub_last_connect(Some(harness_core::workspace_hub::connect_workspace_hub(
        "https://hub.example/probe",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let availability = summary
        .workspace_hub_availability
        .as_deref()
        .expect("workspace hub availability");
    assert!(
        availability.contains("unavailable"),
        "availability={availability}"
    );
    let connect = summary
        .workspace_hub_connect
        .as_deref()
        .expect("workspace hub connect");
    assert!(connect.contains("unavailable"), "connect={connect}");
    assert!(
        rendered.contains("Workspace hub availability:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Workspace hub connect:"),
        "rendered={rendered}"
    );
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_auto_fallback_chain() {
    // Given: live app with auto-fallback chain summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .fallback_chain
        .is_none());

    app.set_auto_fallback_summary(Some(harness_core::auto_fallback::AutoFallbackSummary {
        remaining: 2,
        chain_len: 3,
        exhausted: false,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .fallback_chain
            .as_deref()
            .is_some_and(|text| text.contains("fallback chain:")),
        "expected fallback chain one_line: {:?}",
        summary.fallback_chain
    );
    assert!(
        rendered.contains("Fallback chain: fallback chain:"),
        "expected fallback chain line: {rendered}"
    );
    assert!(
        rendered.contains("2 remaining of 3") && rendered.contains("exhausted=false"),
        "expected remaining/exhausted honesty in fallback chain line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_workspace_hub_bind_upload_recover()
{
    // arrange
    // act
    // assert
    // Given: workspace hub bind/upload/recover diagnostic results bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .workspace_hub_bind
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .workspace_hub_upload
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .workspace_hub_recover
        .is_none());

    app.set_workspace_hub_last_bind(Some(harness_core::workspace_hub::bind_workspace_hub(
        "(probe)",
    )));
    app.set_workspace_hub_last_upload(Some(harness_core::workspace_hub::upload_to_workspace_hub(
        "(probe-artifact)",
    )));
    app.set_workspace_hub_last_recover(Some(harness_core::workspace_hub::recover_workspace_hub(
        "(probe-session)",
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let bind = summary.workspace_hub_bind.as_deref().expect("bind");
    let upload = summary.workspace_hub_upload.as_deref().expect("upload");
    let recover = summary.workspace_hub_recover.as_deref().expect("recover");
    assert!(
        bind.contains("unavailable") || bind.contains("bind"),
        "bind={bind}"
    );
    assert!(
        upload.contains("unavailable") || upload.contains("upload"),
        "upload={upload}"
    );
    assert!(
        recover.contains("unavailable") || recover.contains("recover"),
        "recover={recover}"
    );
    assert!(
        rendered.contains("Workspace hub bind:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Workspace hub upload:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Workspace hub recover:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_fallback_outcome() {
    // arrange
    // act
    // assert
    // Given: last auto-fallback outcome bound (exhausted)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .fallback_outcome
        .is_none());
    app.set_auto_fallback_last_outcome(Some(
        harness_core::auto_fallback::AutoFallbackOutcome::Exhausted {
            failed_model_ref: "p:main".to_string(),
            tried: vec!["p:main".to_string(), "p:fb1".to_string()],
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary
        .fallback_outcome
        .as_deref()
        .expect("fallback outcome line");
    assert!(line.contains("exhausted"), "line={line}");
    assert!(line.contains("p:main"), "line={line}");
    assert!(
        rendered.contains("Fallback outcome:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_fallback_banner() {
    // arrange
    // act
    // assert
    // Given: last auto-fallback banner bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .fallback_banner
        .is_none());
    app.set_auto_fallback_last_banner(Some(
        "provider fallback: (probe):primary → (probe):fb1".to_string(),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary
        .fallback_banner
        .as_deref()
        .expect("fallback banner line");
    assert!(line.contains("provider fallback:"), "line={line}");
    assert!(rendered.contains("Fallback banner:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_fallback_models() {
    // arrange
    // act
    // assert
    // Given: resolved fallback chain label bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .fallback_models
        .is_none());
    app.set_auto_fallback_chain_label(Some(
        "(probe):primary → (probe):fb1 → (probe):fb2".to_string(),
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let line = summary
        .fallback_models
        .as_deref()
        .expect("fallback models line");
    assert!(line.contains("(probe):primary"), "line={line}");
    assert!(rendered.contains("Fallback models:"), "rendered={rendered}");
}

pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_demote_outcome_counts() {
    // Given: live app with demote outcome summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .demote_outcomes
        .is_none());

    app.set_demote_outcome_summary(Some(
        harness_core::foreground_demote::DemoteOutcomeSummary {
            demoted: 1,
            rejected: 1,
            unavailable: 1,
            total: 3,
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .demote_outcomes
            .as_deref()
            .is_some_and(|text| text.contains("demote outcomes:")),
        "expected demote outcomes one_line: {:?}",
        summary.demote_outcomes
    );
    assert!(
        rendered.contains("Demote outcomes: demote outcomes:"),
        "expected demote outcomes line: {rendered}"
    );
    assert!(
        rendered.contains("1 demoted") && rendered.contains("1 rejected"),
        "expected demoted/rejected counts in demote outcomes line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_demote_last() {
    // arrange
    // act
    // assert
    // Given: last demote result bound (shell demote honest unavailable)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).demote_last.is_none());

    let request = harness_core::foreground_demote::DemoteToBackgroundRequest::new(
        "shell-1",
        harness_core::foreground_demote::ForegroundKind::Shell,
    );
    let result = harness_core::foreground_demote::default_demote_policy(&request)
        .expect("default demote policy");
    app.set_demote_last_result(Some(result));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let text = summary.demote_last.as_deref().expect("demote last");
    assert!(
        text.contains("unavailable") || text.contains("demote"),
        "text={text}"
    );
    assert!(rendered.contains("Demote last:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_demote_last_task() {
    // arrange
    // act
    // assert
    // Given: last task-registry demote result bound (rejected)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .demote_last_task
        .is_none());
    app.set_demote_last_task_result(Some(
        harness_core::foreground_demote::DemoteToBackgroundResult::Rejected {
            handle_id: "(probe-task)".to_string(),
            reason: "handle not demotable".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join(
            "
",
        );

    // Then
    let line = summary
        .demote_last_task
        .as_deref()
        .expect("demote last task line");
    assert!(
        line.contains("rejected") || line.contains("(probe-task)"),
        "line={line}"
    );
    assert!(
        rendered.contains("Demote last task:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cron_schedule_counts() {
    // arrange
    // act
    // assert
    // Given: live app with cron schedule summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).cron.is_none());

    app.set_cron_schedule_summary(Some(harness_core::cron_schedule::CronScheduleSummary {
        registered: 2,
        with_label: 1,
        executor_available: false,
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .cron
            .as_deref()
            .is_some_and(|text| text.contains("cron:")),
        "expected cron one_line: {:?}",
        summary.cron
    );
    assert!(
        rendered.contains("Cron: cron:"),
        "expected cron line: {rendered}"
    );
    assert!(
        rendered.contains("executor_available=false"),
        "expected executor_available=false in cron line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cron_register() {
    // arrange
    // act
    // assert
    // Given: cron last-register + first schedule bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .cron_last_register
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .cron_first_schedule
        .is_none());

    let mut registry = harness_core::cron_schedule::CronScheduleRegistry::new();
    let schedule = harness_core::cron_schedule::CronSchedule {
        id: harness_core::cron_schedule::ScheduleId::parse("nightly").expect("schedule id"),
        expression: "0 2 * * *".to_string(),
        label: Some("nightly".to_string()),
        payload_hint: "compact".to_string(),
    };
    let outcome = harness_core::cron_schedule::register_cron_schedule(&mut registry, schedule);
    app.set_cron_last_register(Some(outcome));
    if let Some(first) = registry.list().first() {
        app.set_cron_first_schedule_line(Some(first.one_line()));
    }
    app.set_cron_schedule_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let reg = summary
        .cron_last_register
        .as_deref()
        .expect("cron register");
    assert!(reg.contains("ok") || reg.contains("nightly"), "reg={reg}");
    let first = summary.cron_first_schedule.as_deref().expect("cron first");
    assert!(
        first.contains("nightly") || first.contains("0 2"),
        "first={first}"
    );
    assert!(first.contains("executes=false"), "first={first}");
    assert!(rendered.contains("Cron register:"), "rendered={rendered}");
    assert!(rendered.contains("Cron first:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cron_remove() {
    // arrange
    // act
    // assert
    // Given: cron last-remove fail-closed outcome bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .cron_last_remove
        .is_none());

    let mut registry = harness_core::cron_schedule::CronScheduleRegistry::new();
    let missing = harness_core::cron_schedule::ScheduleId::parse("(missing)").expect("missing id");
    let outcome = harness_core::cron_schedule::remove_cron_schedule(&mut registry, &missing);
    app.set_cron_last_remove(Some(outcome));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let remove = summary.cron_last_remove.as_deref().expect("cron remove");
    assert!(
        remove.contains("failed") || remove.contains("remove"),
        "remove={remove}"
    );
    assert!(rendered.contains("Cron remove:"), "rendered={rendered}");
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_extension_descriptor() {
    // arrange
    // act
    // assert
    // Given: live app with extension manifest summary bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app)
        .extension_descriptor
        .is_none());

    app.set_extension_manifest_summary(Some(
        harness_core::extension_manifest::ExtensionManifestSummary {
            extension_id: "demo.ext".to_string(),
            display_name: Some("Demo".to_string()),
            version: Some("1.0.0".to_string()),
            capabilities: 2,
            enabled_capabilities: 1,
            tools: 3,
            hooks: 0,
            commands: 1,
            prompts: 0,
            mcp_bundles: 0,
            diagnostics: 0,
            provider_decorators: 0,
            loads_external_code: false,
        },
    ));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .extension_descriptor
            .as_deref()
            .is_some_and(|text| text.contains("extension descriptor:")),
        "expected extension descriptor one_line: {:?}",
        summary.extension_descriptor
    );
    assert!(
        rendered.contains("Extension: extension descriptor:"),
        "expected extension line: {rendered}"
    );
    assert!(
        rendered.contains("loads_code=false"),
        "expected honest loads_code=false in extension line: {rendered}"
    );
}

pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_lifecycle_summary() {
    // Given: AppState with plugin lifecycle counts bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert_eq!(
        status_dialog_plugin_summary(&app),
        PluginDialogSummary::default()
    );

    app.set_plugin_lifecycle_summary(Some(harness_core::integrations::PluginLifecycleSummary {
        installed: 3,
        enabled: 1,
        disabled: 2,
    }));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert_eq!(
        summary,
        PluginDialogSummary {
            installed: 3,
            enabled: 1,
            disabled: 2,
            extension_descriptor: None,
            last_install: None,
            last_activate: None,
            last_deactivate: None,
            last_remove: None,
            first_plugin: None,
            discover: None,
            last_load: None,
        }
    );
    assert!(
        rendered.contains("Plugins: 3 installed (1 enabled, 2 disabled)"),
        "expected plugin counts line: {rendered}"
    );

    // When: cleared summary falls back to No Plugins
    app.set_plugin_lifecycle_summary(None);
    let empty = status_dialog_plugin_summary(&app);
    let mut empty_lines = Vec::new();
    append_status_dialog_plugins_section(&mut empty_lines, empty, &theme);
    let empty_rendered = empty_lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        empty_rendered.contains("No Plugins"),
        "expected empty plugins line: {empty_rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_plugin_install() {
    // arrange
    // act
    // assert
    // Given: plugin last-install bound (fail-closed missing package)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app).last_install.is_none());
    assert!(status_dialog_plugin_summary(&app).first_plugin.is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-plugin-install-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let mut registry = harness_core::integrations::PluginLifecycleRegistry::new(&root);
    let outcome = harness_core::integrations::install_plugin_outcome(&mut registry, "(probe)");
    app.set_plugin_last_install(Some(outcome));
    if let Some(first) = registry.list().next() {
        app.set_plugin_first_line(Some(first.one_line()));
    }
    app.set_plugin_lifecycle_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let install = summary.last_install.as_deref().expect("plugin install");
    assert!(
        install.contains("failed") || install.contains("ok"),
        "install={install}"
    );
    assert!(rendered.contains("Plugin install:"), "rendered={rendered}");
    assert!(rendered.contains("Plugin first:"), "rendered={rendered}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_plugin_activate() {
    // arrange
    // act
    // assert
    // Given: plugin last-activate bound (fail-closed missing package)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app).last_activate.is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-plugin-activate-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let mut registry = harness_core::integrations::PluginLifecycleRegistry::new(&root);
    let outcome = harness_core::integrations::activate_plugin_outcome(
        &mut registry,
        "(probe)",
        harness_core::integrations::PluginActivationPermission::Granted,
    );
    app.set_plugin_last_activate(Some(outcome));
    app.set_plugin_lifecycle_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let activate = summary.last_activate.as_deref().expect("plugin activate");
    assert!(
        activate.contains("failed") || activate.contains("ok"),
        "activate={activate}"
    );
    assert!(rendered.contains("Plugin activate:"), "rendered={rendered}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_plugin_deactivate() {
    // arrange
    // act
    // assert
    // Given: plugin last-deactivate bound (fail-closed missing package)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app).last_deactivate.is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-plugin-deactivate-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let mut registry = harness_core::integrations::PluginLifecycleRegistry::new(&root);
    let outcome = harness_core::integrations::deactivate_plugin_outcome(&mut registry, "(probe)");
    app.set_plugin_last_deactivate(Some(outcome));
    app.set_plugin_lifecycle_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let deactivate = summary
        .last_deactivate
        .as_deref()
        .expect("plugin deactivate");
    assert!(
        deactivate.contains("failed") || deactivate.contains("ok"),
        "deactivate={deactivate}"
    );
    assert!(
        rendered.contains("Plugin deactivate:"),
        "rendered={rendered}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_plugin_remove() {
    // arrange
    // act
    // assert
    // Given: plugin last-remove bound (fail-closed missing package)
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app).last_remove.is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-plugin-remove-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let mut registry = harness_core::integrations::PluginLifecycleRegistry::new(&root);
    let outcome = harness_core::integrations::remove_plugin_outcome(&mut registry, "(probe)");
    app.set_plugin_last_remove(Some(outcome));
    app.set_plugin_lifecycle_summary(Some(registry.summary()));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let remove = summary.last_remove.as_deref().expect("plugin remove");
    assert!(
        remove.contains("failed") || remove.contains("ok"),
        "remove={remove}"
    );
    assert!(rendered.contains("Plugin remove:"), "rendered={rendered}");
    let _ = std::fs::remove_dir_all(&root);
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_plugins_section_surfaces_extension_discover() {
    // arrange
    // act
    // assert
    // Given: extension discover + last-load bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_plugin_summary(&app).discover.is_none());
    assert!(status_dialog_plugin_summary(&app).last_load.is_none());

    let root = std::env::temp_dir().join(format!(
        "harness-tui-ext-discover-{}-{}",
        std::process::id(),
        "ws"
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("create workspace");
    let discovered = harness_core::extension_manifest::discover_extension_manifests(&root);
    app.set_extension_discover_summary(Some(
        harness_core::extension_manifest::summarize_extension_discover(&discovered),
    ));
    let probe_path = root
        .join("(probe)")
        .join(harness_core::extension_manifest::EXTENSION_MANIFEST_FILE_NAME);
    app.set_extension_last_load(Some(
        harness_core::extension_manifest::load_extension_manifest_outcome(&probe_path),
    ));

    // When
    let summary = status_dialog_plugin_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_plugins_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let discover = summary.discover.as_deref().expect("extension discover");
    assert!(
        discover.contains("0 descriptor") || discover.contains("descriptor"),
        "discover={discover}"
    );
    let load = summary.last_load.as_deref().expect("extension load");
    assert!(
        load.contains("failed") || load.contains("ok"),
        "load={load}"
    );
    assert!(
        rendered.contains("Extension discover:"),
        "rendered={rendered}"
    );
    assert!(rendered.contains("Extension load:"), "rendered={rendered}");
    let _ = std::fs::remove_dir_all(&root);
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
                status_dialog_body_from_rows(
                    mcp_rows,
                    lsp_rows,
                    PluginDialogSummary::default(),
                    EditAttributionDialogSummary::default(),
                    OperatorDialogSummary::default(),
                    &theme,
                ),
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

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_cow_fastpath() {
    // arrange
    // act
    // assert
    // Given: live app with COW worktree availability bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).cow_fastpath.is_none());

    app.set_cow_worktree_availability(Some(
        harness_core::cow_worktree::CowWorktreeAvailability::Unavailable {
            reason: "probe parent missing for test".to_string(),
            platform: "linux".to_string(),
        },
    ));

    // When: operator summary is built and status dialog is rendered
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then: COW fastpath one_line is surfaced honestly
    assert!(
        summary
            .cow_fastpath
            .as_deref()
            .is_some_and(|t| t.contains("unavailable")),
        "expected cow fastpath one_line: {:?}",
        summary.cow_fastpath
    );
    assert!(
        rendered.contains("COW fastpath:"),
        "expected COW fastpath line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable"),
        "expected unavailable honesty in COW fastpath line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_persistent_graph() {
    // arrange
    // act
    // assert
    // Given: live app with persistent graph availability bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .persistent_graph
        .is_none());

    app.set_persistent_graph_availability(Some(
        harness_core::code_graph::PersistentGraphAvailability::Unavailable {
            reason: "no first-party persistent incremental codebase graph".to_string(),
        },
    ));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .persistent_graph
            .as_deref()
            .is_some_and(|text| text.contains("unavailable")),
        "expected persistent graph one_line: {:?}",
        summary.persistent_graph
    );
    assert!(
        rendered.contains("Persistent graph:"),
        "expected Persistent graph line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable"),
        "expected unavailable honesty in Persistent graph line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_landlock_support() {
    // arrange
    // act
    // assert
    // Given: live app with Landlock support bound for the status dialog
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app).landlock.is_none());

    app.set_landlock_support(Some(harness_core::sandbox::LandlockSupport::Unavailable {
        reason: "Landlock LSM not present in test host list".to_string(),
    }));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    assert!(
        summary
            .landlock
            .as_deref()
            .is_some_and(|text| text.contains("unavailable")),
        "expected landlock one_line: {:?}",
        summary.landlock
    );
    assert!(
        rendered.contains("Landlock:"),
        "expected Landlock line: {rendered}"
    );
    assert!(
        rendered.contains("unavailable"),
        "expected unavailable honesty in Landlock line: {rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_os_sandbox_profiles() {
    // arrange
    // act
    // assert
    // Given: OS sandbox profiles summary bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .os_sandbox_profiles
        .is_none());

    let profiles = harness_core::sandbox::list_os_profiles();
    app.set_os_sandbox_profiles_summary(Some(harness_core::sandbox::summarize_os_profiles(
        &profiles,
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let text = summary
        .os_sandbox_profiles
        .as_deref()
        .expect("OS sandbox profiles");
    assert!(text.contains("total"), "text={text}");
    assert!(
        text.contains("available") || text.contains("unavailable"),
        "text={text}"
    );
    assert!(
        rendered.contains("OS sandbox profiles:"),
        "rendered={rendered}"
    );
}

#[cfg(test)]
#[test]
pub(crate) fn exact_test_status_dialog_operator_summary_surfaces_os_sandbox_first_prepare() {
    // arrange
    // act
    // assert
    // Given: first OS sandbox profile + last prepare bound
    let mut app = AppState::new_live(None, false, None);
    assert!(status_dialog_operator_summary(&app)
        .os_sandbox_first_profile
        .is_none());
    assert!(status_dialog_operator_summary(&app)
        .sandbox_last_prepare
        .is_none());

    let profiles = harness_core::sandbox::list_os_profiles();
    if let Some(first) = profiles.first() {
        app.set_os_sandbox_first_profile_line(Some(first.one_line()));
    }
    app.set_os_sandbox_profiles_summary(Some(harness_core::sandbox::summarize_os_profiles(
        &profiles,
    )));
    app.set_sandbox_last_prepare(Some(harness_core::sandbox::prepare_sandbox(
        harness_core::sandbox::SandboxPolicy::WorkspaceWrite,
    )));

    // When
    let summary = status_dialog_operator_summary(&app);
    let theme = Theme::default();
    let mut lines = Vec::new();
    append_status_dialog_operator_section(&mut lines, summary.clone(), &theme);
    let rendered: String = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");

    // Then
    let first = summary
        .os_sandbox_first_profile
        .as_deref()
        .expect("first profile");
    assert!(
        first.contains("OS sandbox profile") || first.contains("policy="),
        "first={first}"
    );
    let prepare = summary
        .sandbox_last_prepare
        .as_deref()
        .expect("sandbox prepare");
    assert!(
        prepare.contains("sandbox prepare")
            || prepare.contains("unavailable")
            || prepare.contains("not_required")
            || prepare.contains("prepared"),
        "prepare={prepare}"
    );
    assert!(
        rendered.contains("OS sandbox first profile:"),
        "rendered={rendered}"
    );
    assert!(
        rendered.contains("Sandbox prepare last:"),
        "rendered={rendered}"
    );
}
