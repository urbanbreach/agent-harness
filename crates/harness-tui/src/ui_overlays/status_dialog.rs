use std::collections::BTreeSet;

use harness_core::event::EventV1;

use super::*;

#[path = "status_dialog/agents.rs"]
mod agents;
use agents::{render_dashboard_peek, render_dashboard_roster};

pub(crate) fn render_status_dashboard_surface(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let Some(dashboard) = app.status_dashboard() else {
        return;
    };
    let Some(surface) = crate::dashboard_integration::dashboard_viewport(root) else {
        return;
    };
    if !paint_overlay_panel_titled(frame, theme, surface, "Status · Harness dashboard", None) {
        return;
    }
    let Some(content) = crate::dashboard_integration::dashboard_content_viewport(root) else {
        return;
    };

    render_interactive_dashboard(frame, app, theme, surface);
    if let Some(details) = dashboard.layout().details.filter(|area| area.height >= 13) {
        render_dashboard_summary(
            frame,
            app,
            theme,
            Rect::new(
                details.x.saturating_add(1),
                details.bottom().saturating_sub(4),
                details.width.saturating_sub(2),
                3,
            ),
        );
    }
    if dashboard.help_visible() {
        render_dashboard_help(frame, theme, content, dashboard);
    }
}

fn render_interactive_dashboard(frame: &mut Frame, app: &AppState, theme: &Theme, overlay: Rect) {
    let Some(dashboard) = app.status_dashboard() else {
        return;
    };
    let layout = dashboard.layout();
    render_dashboard_roster(frame, app, theme, layout.roster, dashboard);
    render_dashboard_peek(frame, app, theme, layout.peek, dashboard);
    render_dashboard_reply(frame, app, theme, layout.reply, dashboard);
    if let Some(details) = layout.details {
        render_dashboard_details(frame, theme, details, dashboard);
    }
    let focus = if dashboard.search_state().context.is_some() {
        format!("/{}", dashboard.search_state().query)
    } else {
        "↑↓ select · Tab focus · / search · d details · h help · Esc close".to_string()
    };
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

fn render_dashboard_reply(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let focused = dashboard.focus() == crate::dashboard_integration::DashboardPane::Reply;
    let border = if focused {
        theme.text.accent
    } else {
        theme.terminal_colors.muted
    };
    let panel = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .title("Reply");
    let inner = panel.inner(area);
    frame.render_widget(panel, area);
    let selected_current = dashboard
        .roster_state()
        .selected_key()
        .map(|key| key.as_str())
        == app.run_id();
    if selected_current && app.active_permission_view().is_some() {
        let message = if dashboard.layout().peek.height == 0 {
            "Input required · Enter to review"
        } else {
            "Answer above · Enter opens full review"
        };
        frame.render_widget(
            Paragraph::new(message).style(Style::default().fg(theme.text.accent)),
            inner,
        );
        return;
    }
    let text = dashboard
        .reply_editor()
        .map_or_else(String::new, |editor| editor.text());
    if text.is_empty() {
        frame.render_widget(
            Paragraph::new("Write a reply…").style(Style::default().fg(theme.text.secondary)),
            inner,
        );
        if focused && inner.width > 0 && inner.height > 0 {
            frame.set_cursor_position((inner.x, inner.y));
        }
        return;
    }
    let byte = dashboard.reply_editor().map_or(text.len(), |editor| {
        use unicode_segmentation::UnicodeSegmentation;
        text.graphemes(true)
            .take(editor.cursor().insertion_index())
            .map(str::len)
            .sum()
    });
    let Ok(wrapped) =
        crate::transcript_selection::TextLayout::new(text, usize::from(inner.width.max(1)))
    else {
        return;
    };
    let cursor = wrapped.point_for_byte(byte);
    let top = cursor
        .row
        .saturating_sub(usize::from(inner.height.saturating_sub(1)));
    let lines = (top..wrapped.row_count())
        .map(|row| Line::from(wrapped.row_text(row)))
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme.text.primary)),
        inner,
    );
    if focused && inner.width > 0 && inner.height > 0 {
        frame.set_cursor_position((
            inner.x
                + u16::try_from(cursor.cell)
                    .unwrap_or(u16::MAX)
                    .min(inner.width - 1),
            inner.y
                + u16::try_from(cursor.row.saturating_sub(top))
                    .unwrap_or(u16::MAX)
                    .min(inner.height - 1),
        ));
    }
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
    render_dashboard_pane(
        frame,
        theme,
        area,
        "Details",
        lines,
        if area.height >= 13 { 4 } else { 0 },
    );
}

fn render_dashboard_pane(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    title: &str,
    lines: Vec<String>,
    footer_rows: u16,
) {
    let surface = theme.surface.canvas;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.terminal_colors.muted).bg(surface))
        .style(Style::default().bg(surface))
        .title(title);
    let mut inner = block.inner(area);
    inner.height = inner.height.saturating_sub(footer_rows);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines.join("\n"))
            .style(Style::default().fg(theme.text.primary).bg(surface))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

fn render_dashboard_summary(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    let banner = app.status_banner.as_deref().unwrap_or_default().trim();
    let lower = banner.to_ascii_lowercase();
    let provider_fallback = lower.contains("provider fallback");
    let crash = !provider_fallback
        && ["previous crash", "recovery", "action:", "stale writer"]
            .iter()
            .any(|word| lower.contains(word));
    let probes = [
        crash,
        provider_fallback,
        app.auto_fallback_summary().is_some(),
        app.auto_fallback_last_outcome().is_some(),
        app.auto_fallback_last_banner().is_some(),
        app.auto_fallback_chain_label().is_some(),
        app.focused_demote_handle_id().is_some(),
        app.demote_outcome_summary().is_some(),
        app.demote_last_result().is_some(),
        app.demote_last_task_result().is_some(),
        app.settings_project_config_path().is_some() && !app.replay_mode,
        app.settings_registry_summary().is_some(),
        app.crash_recovery_scan_summary().is_some(),
        app.crash_recovery_first_report().is_some(),
        app.crash_recovery_resolved_action().is_some(),
        app.crash_recovery_first_report_line().is_some(),
        app.team_registry_summary().is_some(),
        app.team_last_create().is_some(),
        app.team_first_line().is_some(),
        app.team_last_send().is_some(),
        app.team_last_message_line().is_some(),
        app.team_last_add_member().is_some(),
        app.team_last_cancel().is_some(),
        app.cron_schedule_summary().is_some(),
        app.cron_last_register().is_some(),
        app.cron_first_schedule_line().is_some(),
        app.cron_last_remove().is_some(),
        app.workspace_hub_availability().is_some(),
        app.graph_query_batch_summary().is_some(),
        app.graph_query_last_result().is_some(),
        app.graph_query_batch_first_line().is_some(),
        app.persistent_graph_availability().is_some(),
        app.cow_clone_outcome_summary().is_some(),
        app.cow_clone_last_result().is_some(),
        app.cow_worktree_availability().is_some(),
        app.browser_oidc_availability().is_some(),
        app.mcp_oauth_remote_availability().is_some(),
        app.sleep_wake_observation_summary().is_some(),
        app.sleep_wake_credential_policy().is_some(),
        app.sleep_wake_last_observation().is_some(),
        app.sleep_wake_last_decision().is_some(),
        app.sleep_wake_availability().is_some(),
        app.binary_update_summary().is_some(),
        app.binary_update_policy().is_some(),
        app.binary_update_check().is_some(),
        app.binary_version_info().is_some(),
        app.foreign_discover_summary().is_some(),
        app.foreign_import_first_candidate().is_some(),
        app.foreign_import_last_outcome().is_some(),
        app.jujutsu_probe().is_some(),
        app.jujutsu_cli().is_some(),
        app.jujutsu_workspace().is_some(),
        app.jujutsu_last_command().is_some(),
        app.sandbox_fs_plan_summary().is_some(),
        app.landlock_support().is_some(),
        app.os_sandbox_profiles_summary().is_some(),
        app.os_sandbox_first_profile_line().is_some(),
        app.sandbox_last_prepare().is_some(),
        app.acp_connection_summary().is_some(),
        app.acp_connection_state().is_some(),
        app.acp_session_info().is_some(),
        app.acp_last_connect().is_some(),
        app.acp_last_bind().is_some(),
        app.edit_attribution_summary().is_some(),
        app.edit_attribution_first_line().is_some(),
        app.edit_attribution_last_line().is_some(),
        true, // A plan summary is available even when the list is empty.
        !app.plan_entries.is_empty(),
    ];
    let bound = probes.iter().filter(|present| **present).count();
    let operator_line = format!(
        "operator dashboard: {bound} bound of {} probes",
        probes.len()
    );
    let plugins = app.plugin_lifecycle_summary();
    let plugin_line = format!(
        "Plugins: {} installed ({} enabled, {} disabled)",
        plugins.map_or(0, |value| value.installed),
        plugins.map_or(0, |value| value.enabled),
        plugins.map_or(0, |value| value.disabled),
    );
    let edits = app
        .events
        .iter()
        .filter_map(|event| match &event.payload {
            EventV1::EditApplied(edit) => Some(edit.path.as_str()),
            _ => None,
        })
        .collect::<BTreeSet<_>>()
        .len();
    let edit_line = if edits == 0 {
        "Edit attribution: none yet".to_string()
    } else {
        format!("Edit attribution: {edits} edits")
    };
    let mcp = if harness_core::config::registered_integrations_config()
        .is_some_and(|config| !config.mcp.servers.is_empty())
    {
        "MCP servers available"
    } else {
        "No MCP Servers"
    };
    let crash_line = if crash {
        format!("Crash/recovery: {}", sanitize_status_dialog_text(banner))
    } else {
        "Crash/recovery: none".to_string()
    };
    let fallback_line = app
        .auto_fallback_last_banner()
        .map_or_else(String::new, |value| {
            format!("Fallback banner: {}", sanitize_status_dialog_text(value))
        });
    let width = usize::from(area.width);
    let lines = [
        truncate_plain_text(&format!("Operator · {mcp} · {plugin_line}"), width),
        truncate_plain_text(&format!("{edit_line} · {operator_line}"), width),
        status_summary_pair(&crash_line, &fallback_line, width),
    ];
    frame.render_widget(
        Paragraph::new(lines.join("\n")).style(Style::default().fg(theme.text.secondary)),
        area,
    );
}

fn status_summary_pair(left: &str, right: &str, width: usize) -> String {
    if right.is_empty() {
        return truncate_plain_text(left, width);
    }

    const SEPARATOR: &str = " · ";
    let available = width.saturating_sub(SEPARATOR.len());
    let right_width = available / 2;
    let left_width = available.saturating_sub(right_width);
    format!(
        "{}{}{}",
        truncate_plain_text(left, left_width),
        SEPARATOR,
        truncate_plain_text(right, right_width)
    )
}

fn render_dashboard_help(
    frame: &mut Frame,
    theme: &Theme,
    area: Rect,
    dashboard: &crate::dashboard_integration::DashboardIntegration,
) {
    let help = dashboard.focused_help();
    let lines = help
        .entries
        .into_iter()
        .map(|entry| format!("{}  {}", entry.key, entry.action))
        .collect::<Vec<_>>();
    render_dashboard_pane(frame, theme, area, "Dashboard help", lines, 0);
}

fn dashboard_status_label(status: crate::dashboard::DashboardStatus) -> &'static str {
    match status {
        crate::dashboard::DashboardStatus::AwaitingInput => "needs input",
        crate::dashboard::DashboardStatus::Running => "working",
        crate::dashboard::DashboardStatus::Queued => "queued",
        crate::dashboard::DashboardStatus::Streaming => "streaming",
        crate::dashboard::DashboardStatus::Completed => "settled",
        crate::dashboard::DashboardStatus::Failed => "failed",
        crate::dashboard::DashboardStatus::Cancelled => "stopped",
        crate::dashboard::DashboardStatus::Stale => "stale",
    }
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
