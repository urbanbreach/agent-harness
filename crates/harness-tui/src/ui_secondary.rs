use super::*;

pub(super) fn render_replay_secondary_column(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let [activity_area, inspector_area] = details_drawer_areas(area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel)),
        area,
    );
    render_activity_pane(frame, app, activity_area, theme);
    render_inspector_pane(frame, app, inspector_area, theme);
}

pub(super) fn render_activity_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);

    let title = if app.replay_mode {
        format!(
            "Replay index · {} turn{}",
            app.activities.len(),
            if app.activities.len() == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "Activity (j/k active{}{})",
            if app.follow_mode { ", follow" } else { "" },
            if is_focused { ", focused" } else { "" }
        )
    };

    let surface = if app.replay_mode {
        theme.surface.panel
    } else {
        theme.surface.panel_elevated
    };
    let block = panel_block(theme, title, is_focused, surface);

    if app.activities.is_empty() {
        let empty = Paragraph::new(if app.replay_mode {
            "No recorded turns"
        } else {
            "No activities yet"
        })
        .block(block)
        .style(panel_style(surface, theme.text.secondary));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .activities
        .iter()
        .enumerate()
        .map(|(idx, activity)| {
            let is_selected = idx == app.selected_activity_index;
            let prefix = if is_selected { "> " } else { "  " };
            let status_icon = match activity.status {
                ActivityStatus::Streaming => theme.live_shell.glyphs.streaming,
                ActivityStatus::Done => theme.live_shell.glyphs.done,
                ActivityStatus::Error => theme.live_shell.glyphs.error,
            };
            let status_text = match activity.status {
                ActivityStatus::Streaming => "streaming…",
                ActivityStatus::Done => "done",
                ActivityStatus::Error => "error",
            };

            let style = if is_selected {
                Style::default()
                    .fg(theme.text.inverse)
                    .bg(theme.border.focus)
                    .add_modifier(Modifier::BOLD)
            } else {
                panel_style(surface, theme.text.primary)
            };

            let model_display = if activity.model_id.is_empty() {
                "-"
            } else {
                &activity.model_id
            };
            let request_id = request_id_label(&activity.request_id);

            let content = if app.replay_mode {
                format!(
                    "{}{} · {} · {} {}",
                    prefix, request_id, status_text, model_display, status_icon
                )
            } else {
                format!(
                    "{}{} {} {} {}",
                    prefix, request_id, model_display, status_text, status_icon
                )
            };
            Line::from(Span::styled(content, style))
        })
        .collect();

    let activity_list = Paragraph::new(Text::from(items))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: false });

    frame.render_widget(activity_list, area);
}

pub(super) fn render_live_details_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    render_details_drawer(frame, app, overlay, theme);
}

pub(super) fn render_inspector_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && activity_surface_visible(app);

    let title = if app.replay_mode {
        format!(
            "Selection · read-only{}",
            if is_focused { " · focus" } else { "" }
        )
    } else {
        format!("Inspector{}", if is_focused { " (focused)" } else { "" })
    };
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);
    let content = if app.replay_mode && app.activities.is_empty() {
        Text::from("Replay is read-only\nPick a recorded turn or open Events / Diff.")
    } else {
        build_inspector_content(app, theme)
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .scroll((app.details_scroll, 0))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

pub(super) fn render_events_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, events_summary_line(app, theme));
    let [event_list_area, event_details_area] =
        split_secondary_surface(body, 34, theme.live_shell.rhythm.surface_gap);

    render_event_list(frame, app, event_list_area, theme);
    render_event_details(frame, app, event_details_area, theme);
}

pub(super) fn render_diff_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, diff_summary_line(app, theme));
    let [event_list_area, diff_area] =
        split_secondary_surface(body, 34, theme.live_shell.rhythm.surface_gap);

    render_event_list(frame, app, event_list_area, theme);

    let is_focused = app.focus == Focus::Details;
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, "Diff", is_focused, surface);

    let content = if let Some(path) = &app.session_path {
        if let Some(event) = app.selected_event() {
            if let Some(diff_content) = load_diff_for_event(path, event) {
                diff_content
            } else {
                format!("diff artifact missing:\n{}", path.display())
            }
        } else {
            "Select an edit event to view diff".to_string()
        }
    } else {
        "No session loaded".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, diff_area);
}

pub(super) fn render_help_tab(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let body = render_secondary_surface_shell(frame, area, theme, help_summary_line(app, theme));
    let surface = theme.surface.panel_elevated;
    let block = panel_block(
        theme,
        if app.replay_mode { "Reference" } else { "Help" },
        false,
        surface,
    );

    let paragraph = Paragraph::new(help_text(app))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, body);
}

#[cfg(test)]
pub(crate) fn orchestration_card_text_for_test(
    app: &AppState,
    height: u16,
    width: u16,
) -> Vec<String> {
    orchestration_card_lines(
        app,
        &app.orchestration_visible_rows(),
        app.theme(),
        height,
        width,
    )
    .into_iter()
    .map(|line| {
        line.spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>()
    })
    .collect()
}

fn help_row(app: &AppState, action: Action, label: &str) -> String {
    format!("  {:<12} {label}", app.keymap.get_binding_str(action))
}

fn help_text(app: &AppState) -> String {
    let mut lines = vec![
        "Keyboard Shortcuts:".to_string(),
        String::new(),
        "Navigation:".to_string(),
        help_row(app, Action::MoveDown, "Move down in list"),
        help_row(app, Action::MoveUp, "Move up in list"),
        help_row(app, Action::FocusNext, "Cycle focus forward"),
        help_row(app, Action::FocusPrev, "Cycle focus backward"),
        help_row(app, Action::ToggleFollow, "Toggle follow mode"),
    ];

    if app.replay_mode {
        lines.extend([
            String::new(),
            "Replay surfaces:".to_string(),
            help_row(app, Action::TabRun, "Open conversation"),
            help_row(app, Action::TabEvents, "Open Events surface"),
            help_row(app, Action::TabDiff, "Open Diff surface"),
            help_row(app, Action::TabHelp, "Open Help surface"),
        ]);
    } else {
        lines.extend([
            String::new(),
            "Live surfaces:".to_string(),
            help_row(app, Action::TabRun, "Return to conversation"),
            help_row(app, Action::ToggleDetailsDrawer, "Toggle details drawer"),
            help_row(app, Action::TabEvents, "Open Events surface"),
            help_row(app, Action::TabDiff, "Open Diff surface"),
            help_row(app, Action::TabHelp, "Open Help surface"),
            String::new(),
            "Prompt (when focused):".to_string(),
            help_row(app, Action::SubmitPrompt, "Submit prompt"),
            help_row(app, Action::InsertNewline, "Insert newline"),
            help_row(app, Action::ClearPrompt, "Clear prompt"),
            help_row(app, Action::HistoryUp, "History up"),
            help_row(app, Action::HistoryDown, "History down"),
        ]);
    }

    lines.extend([
        String::new(),
        "Permission modal:".to_string(),
        help_row(app, Action::AllowPermission, "Allow permission"),
        help_row(app, Action::DenyPermission, "Deny permission"),
        help_row(app, Action::DismissModal, "Dismiss modal"),
        String::new(),
        "General:".to_string(),
        help_row(app, Action::Help, "Show this help"),
    ]);

    if app.replay_mode {
        lines.push(help_row(app, Action::Reload, "Reload session"));
    }

    lines.push(help_row(app, Action::Quit, "Quit"));
    lines.join("\n")
}

fn render_details_drawer(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let drawer_chunks = details_drawer_areas(area);

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.panel)),
        area,
    );
    render_details_orchestration_card(frame, app, drawer_chunks[0], theme);
    render_details_inspector_card(frame, app, drawer_chunks[1], theme);
}

fn activity_surface_visible(app: &AppState) -> bool {
    (app.replay_mode && app.active_tab == Tab::Run) || app.details_drawer_open()
}

fn orchestration_title_meta(app: &AppState) -> String {
    let summary = app.orchestration_summary();
    let tracked = app.orchestration_visible_rows().len();
    let warning_count = usize::from(app.orchestration_latest_warning().is_some());
    format!(
        "{tracked} tracked · {} active · {warning_count} warn",
        summary.active_agents
    )
}

fn inspector_title_meta(app: &AppState) -> Option<String> {
    let activity = app.activities.get(app.selected_activity_index)?;
    Some(format!(
        "{} · {}",
        request_id_label(&activity.request_id),
        if activity.tool_calls.is_empty() {
            activity.status.to_string()
        } else {
            format!(
                "{} tool{}",
                activity.tool_calls.len(),
                if activity.tool_calls.len() == 1 {
                    ""
                } else {
                    "s"
                }
            )
        }
    ))
}

fn render_details_orchestration_card(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List && activity_surface_visible(app);
    let surface = if is_focused {
        theme.surface.overlay
    } else {
        theme.surface.panel_elevated
    };
    let [title_area, body_area] = details_section_areas(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    render_details_section_title(
        frame,
        title_area,
        theme,
        surface,
        "Orchestration",
        Some(&orchestration_title_meta(app)),
        is_focused,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let rows = app.orchestration_visible_rows();
    let visible_rows =
        orchestration_card_lines(app, &rows, theme, body_area.height, body_area.width);

    frame.render_widget(
        Paragraph::new(Text::from(visible_rows)).style(panel_style(surface, theme.text.primary)),
        body_area,
    );
}

fn orchestration_card_lines(
    app: &AppState,
    rows: &[OrchestrationTaskRow],
    theme: &Theme,
    height: u16,
    width: u16,
) -> Vec<Line<'static>> {
    if height == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    lines.push(orchestration_summary_line(app, theme, width));

    if height == 1 {
        return lines;
    }

    lines.push(orchestration_warning_line(app, theme, width));
    let task_slots = usize::from(height.saturating_sub(2));
    if task_slots == 0 || rows.is_empty() {
        return lines;
    }

    if rows.len() <= task_slots {
        lines.extend(
            rows.iter()
                .map(|row| orchestration_task_line(app, row, theme, width)),
        );
        return lines;
    }

    if task_slots == 1 {
        lines.push(orchestration_overflow_line(rows.len(), theme));
        return lines;
    }

    let visible_task_count = task_slots.saturating_sub(1);
    lines.extend(
        rows.iter()
            .take(visible_task_count)
            .map(|row| orchestration_task_line(app, row, theme, width)),
    );
    lines.push(orchestration_overflow_line(
        rows.len().saturating_sub(visible_task_count),
        theme,
    ));
    lines
}

fn orchestration_summary_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let summary = app.orchestration_summary();
    let text = format!(
        "overview · {} active agents · {} queued · {} running · {} stale",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        muted_meta_style(theme),
    ))
}

fn orchestration_warning_line(app: &AppState, theme: &Theme, width: u16) -> Line<'static> {
    let warning = app.orchestration_latest_warning().unwrap_or("none");
    let text = format!("watch · {warning}");
    Line::from(Span::styled(
        truncate_plain_text(&text, usize::from(width)),
        Style::default().fg(theme.status.warning),
    ))
}

fn orchestration_task_line(
    app: &AppState,
    row: &OrchestrationTaskRow,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let (state_label, state_color) = orchestration_state_tokens(row.state, theme);
    let owner = app.orchestration_owner_labels(row);
    let queue_key = row.queue_key.as_deref().unwrap_or("queue:none");
    let detail = format!(
        "{} · {}/{} · {}",
        row.task_id, owner.label, owner.profile, queue_key
    );

    let badge_width = state_label.chars().count().saturating_add(4);
    let detail = truncate_plain_text(&detail, usize::from(width).saturating_sub(badge_width));

    Line::from(vec![
        status_badge(state_label, state_color, theme),
        Span::raw(" "),
        Span::styled(detail, muted_meta_style(theme)),
    ])
}

fn orchestration_overflow_line(hidden_count: usize, theme: &Theme) -> Line<'static> {
    Line::from(Span::styled(
        format!("+{hidden_count} more"),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    ))
}

fn orchestration_state_tokens(
    state: OrchestrationTaskState,
    theme: &Theme,
) -> (&'static str, Color) {
    match state {
        OrchestrationTaskState::Queued => ("queued", theme.text.secondary),
        OrchestrationTaskState::Running => ("running", theme.status.info),
        OrchestrationTaskState::Stale => ("stale", theme.status.warning),
        OrchestrationTaskState::Completed => ("completed", theme.status.success),
        OrchestrationTaskState::Cancelled => ("cancelled", theme.status.error),
        OrchestrationTaskState::LateResult => ("late-result", theme.status.warning),
    }
}

fn build_inspector_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let runtime_state = app.runtime_state();

    if let Some(activity) = app.activities.get(app.selected_activity_index) {
        let mut lines = Vec::new();

        if let Some(detail) = runtime_state.detail.clone() {
            lines.push(Line::from(vec![
                Span::styled("Runtime ", Style::default().add_modifier(Modifier::BOLD)),
                status_badge(
                    runtime_state.kind.label(),
                    runtime_state_color(runtime_state.kind, theme),
                    theme,
                ),
            ]));
            lines.push(Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )));
            lines.push(Line::from(""));
        }

        append_section_header(&mut lines, "Activity metadata:", theme.text.primary);
        append_labeled_value(
            &mut lines,
            "  Request ID: ",
            request_id_label(&activity.request_id),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Provider: ",
            activity.provider_id.clone(),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Model: ",
            activity.model_id.clone(),
            theme.text.primary,
        );
        append_labeled_value(
            &mut lines,
            "  Status: ",
            activity.status.to_string(),
            match activity.status {
                crate::app::ActivityStatus::Error => theme.status.error,
                crate::app::ActivityStatus::Done => theme.status.success,
                _ => theme.text.primary,
            },
        );
        append_labeled_value(
            &mut lines,
            "  Sequences: ",
            format!("{}-{}", activity.first_seq, activity.last_seq),
            theme.text.primary,
        );

        if let Some(req_data) = &activity.request_data {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Request metadata:", theme.text.primary);
            append_detail_payload(
                &mut lines,
                "  Prompt summary:",
                &req_data.prompt_summary,
                theme.text.primary,
            );
            append_labeled_value(
                &mut lines,
                "  Request digest: ",
                req_data.request_digest.clone(),
                theme.text.secondary,
            );
            match serde_json::to_string_pretty(req_data) {
                Ok(json) => {
                    append_detail_payload(&mut lines, "  Raw request:", &json, theme.text.primary)
                }
                Err(_) => append_labeled_value(
                    &mut lines,
                    "  Raw request: ",
                    "[error serializing]",
                    theme.status.error,
                ),
            }
        }

        if !activity.permissions.is_empty() {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Permission context:", theme.text.primary);
            append_permission_details(&mut lines, &activity.permissions, theme, "  ");
        }

        if !activity.tool_calls.is_empty() {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Tool calls:", theme.text.primary);
            append_tool_call_details(&mut lines, &activity.tool_calls, theme);
        }

        if let Some(error) = &activity.error_message {
            lines.push(Line::from(""));
            append_section_header(&mut lines, "Runtime errors:", theme.status.error);
            append_detail_payload(&mut lines, "  Raw error:", error, theme.status.error);
        }

        Text::from(lines)
    } else if let Some(detail) = runtime_state.detail.clone() {
        Text::from(vec![
            Line::from(vec![
                Span::styled("Runtime ", Style::default().add_modifier(Modifier::BOLD)),
                status_badge(
                    runtime_state.kind.label(),
                    runtime_state_color(runtime_state.kind, theme),
                    theme,
                ),
            ]),
            Line::from(Span::styled(
                detail,
                Style::default().fg(theme.text.secondary),
            )),
        ])
    } else {
        Text::from("No activity selected")
    }
}

fn build_compact_inspector_content(app: &AppState, theme: &Theme) -> Text<'static> {
    let Some(activity) = app.activities.get(app.selected_activity_index) else {
        return build_inspector_content(app, theme);
    };

    let mut lines = Vec::new();
    append_labeled_value(
        &mut lines,
        "Request ID: ",
        request_id_label(&activity.request_id),
        theme.text.primary,
    );
    append_labeled_value(
        &mut lines,
        "Provider: ",
        activity.provider_id.clone(),
        theme.text.primary,
    );
    append_labeled_value(
        &mut lines,
        "Model: ",
        activity.model_id.clone(),
        theme.text.primary,
    );
    if let Some(req_data) = &activity.request_data {
        append_labeled_value(
            &mut lines,
            "Prompt summary: ",
            req_data.prompt_summary.clone(),
            theme.text.primary,
        );
    } else {
        append_labeled_value(
            &mut lines,
            "Status: ",
            activity.status.to_string(),
            theme.text.primary,
        );
    }
    Text::from(lines)
}

fn render_details_inspector_card(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details && activity_surface_visible(app);
    let surface = if is_focused {
        theme.surface.overlay
    } else {
        theme.surface.panel_elevated
    };
    let [title_area, body_area] = details_section_areas(area);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);
    render_details_section_title(
        frame,
        title_area,
        theme,
        surface,
        "Details",
        inspector_title_meta(app).as_deref(),
        is_focused,
    );

    frame.render_widget(
        Paragraph::new(if body_area.height <= 6 {
            build_compact_inspector_content(app, theme)
        } else {
            build_inspector_content(app, theme)
        })
        .style(panel_style(surface, theme.text.primary))
        .scroll((app.details_scroll, 0))
        .wrap(Wrap { trim: true }),
        body_area,
    );
}

fn details_section_areas(area: Rect) -> [Rect; 2] {
    let inner = inset_rect(area, 1, 0);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(inner);
    [chunks[0], chunks[1]]
}

fn render_details_section_title(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    surface: Color,
    title: &str,
    meta: Option<&str>,
    is_focused: bool,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let indicator = if is_focused { "●" } else { "○" };
    let indicator_color = if is_focused {
        theme.text.accent
    } else {
        theme.text.tertiary
    };

    let mut spans = vec![
        Span::styled(
            format!("{indicator} "),
            Style::default().fg(indicator_color).bg(surface),
        ),
        Span::styled(
            title.to_string(),
            Style::default()
                .fg(if is_focused {
                    theme.text.primary
                } else {
                    theme.text.secondary
                })
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(meta) = meta {
        spans.push(Span::styled(
            format!(" · {meta}"),
            Style::default().fg(theme.text.tertiary).bg(surface),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn append_section_header<'a>(lines: &mut Vec<Line<'a>>, title: &str, color: Color) {
    lines.push(Line::from(Span::styled(
        title.to_string(),
        Style::default().fg(color).add_modifier(Modifier::BOLD),
    )));
}

fn append_labeled_value<'a>(
    lines: &mut Vec<Line<'a>>,
    label: &str,
    value: impl Into<String>,
    color: Color,
) {
    lines.push(Line::from(vec![
        Span::styled(
            label.to_string(),
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.into(), Style::default().fg(color)),
    ]));
}

fn permission_decision_label(decision: harness_core::event::PermissionDecision) -> &'static str {
    match decision {
        harness_core::event::PermissionDecision::Allow => "allow",
        harness_core::event::PermissionDecision::Deny => "deny",
    }
}

fn permission_status_style(
    permission: &crate::app::PermissionEntry,
    theme: &Theme,
) -> (&'static str, &'static str, Color) {
    match permission.resolved_decision {
        Some(harness_core::event::PermissionDecision::Allow) => (
            "allowed",
            theme.live_shell.glyphs.succeeded,
            theme.status.success,
        ),
        Some(harness_core::event::PermissionDecision::Deny) => {
            ("denied", theme.live_shell.glyphs.failed, theme.status.error)
        }
        None => (
            "pending",
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
        ),
    }
}

fn append_permission_details(
    lines: &mut Vec<Line<'static>>,
    permissions: &[crate::app::PermissionEntry],
    theme: &Theme,
    indent: &str,
) {
    for permission in permissions {
        let (status_label, status_icon, status_color) = permission_status_style(permission, theme);

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{indent}{status_icon} "),
                Style::default().fg(status_color),
            ),
            Span::styled(
                permission.permission_id.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" · {} · {}", permission.kind, status_label),
                Style::default().fg(theme.text.secondary),
            ),
        ]));
        append_labeled_value(
            lines,
            &format!("{indent}Summary: "),
            permission.summary.clone(),
            theme.text.primary,
        );
        if let Some(tool_call_id) = &permission.tool_call_id {
            append_labeled_value(
                lines,
                &format!("{indent}Tool call: "),
                tool_call_id.clone(),
                theme.text.secondary,
            );
        }
        append_labeled_value(
            lines,
            &format!("{indent}Request digest: "),
            permission.request_digest.clone(),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Timeout: "),
            format!("{} ms", permission.timeout_ms),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Default: "),
            permission_decision_label(permission.default_decision),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            &format!("{indent}Sequences: "),
            format!("{}-{}", permission.first_seq, permission.last_seq),
            theme.text.secondary,
        );
        if let Some(decision) = permission.resolved_decision {
            append_labeled_value(
                lines,
                &format!("{indent}Resolved: "),
                permission_decision_label(decision),
                status_color,
            );
        }
        if let Some(reason) = &permission.resolution_reason {
            append_detail_payload(
                lines,
                &format!("{indent}Reason:"),
                reason,
                theme.text.primary,
            );
        }
    }
}

fn append_tool_call_details(
    lines: &mut Vec<Line<'static>>,
    tool_calls: &[crate::app::ToolCallEntry],
    theme: &Theme,
) {
    for tool_call in tool_calls {
        let (status_icon, status_color) = match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => (
                theme.live_shell.glyphs.pending_permission,
                theme.status.warning,
            ),
            ToolCallDisplayStatus::Queued => (theme.live_shell.glyphs.queued, theme.text.secondary),
            ToolCallDisplayStatus::Running => (theme.live_shell.glyphs.running, theme.text.accent),
            ToolCallDisplayStatus::Succeeded => {
                (theme.live_shell.glyphs.succeeded, theme.status.success)
            }
            ToolCallDisplayStatus::Failed => (theme.live_shell.glyphs.failed, theme.status.error),
        };

        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", status_icon),
                Style::default().fg(status_color),
            ),
            Span::styled("tool ", Style::default().fg(theme.text.secondary)),
            Span::styled(
                tool_call.tool_id.clone(),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            tool_status_badge(tool_call.status, theme),
        ]));
        append_labeled_value(
            lines,
            "  Call ID: ",
            tool_call.tool_call_id.clone(),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            "  State: ",
            tool_call.status.to_string(),
            status_color,
        );
        append_labeled_value(
            lines,
            "  Sequences: ",
            format!("{}-{}", tool_call.first_seq, tool_call.last_seq),
            theme.text.secondary,
        );
        append_labeled_value(
            lines,
            "  Args digest: ",
            tool_call.args_digest.clone(),
            theme.text.secondary,
        );
        append_detail_payload(
            lines,
            "  Args:",
            &tool_call.args_summary,
            theme.text.primary,
        );

        if !tool_call.permissions.is_empty() {
            lines.push(Line::from(Span::styled(
                "  Permission context:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            append_permission_details(lines, &tool_call.permissions, theme, "    ");
        }

        if let Some(output) = &tool_call.output_summary {
            if let Some(output_digest) = &tool_call.output_digest {
                append_labeled_value(
                    lines,
                    "  Output digest: ",
                    output_digest.clone(),
                    theme.text.secondary,
                );
            }
            let (label, color) = if tool_call.status == ToolCallDisplayStatus::Failed {
                ("  Error:", theme.status.error)
            } else {
                ("  Result:", theme.text.primary)
            };
            append_detail_payload(lines, label, output, color);
        }
    }
}

fn append_detail_payload<'a>(lines: &mut Vec<Line<'a>>, label: &str, payload: &str, color: Color) {
    lines.push(Line::from(Span::styled(
        label.to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    )));
    append_text_block(lines, &format_detail_payload(payload), color, "    ");
}

pub(crate) fn format_detail_payload(payload: &str) -> String {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => serde_json::to_string_pretty(&value).unwrap_or_else(|_| trimmed.to_string()),
        Err(_) => trimmed.to_string(),
    }
}

fn render_event_list(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::List;

    let follow_indicator = if app.follow_mode { " · follow" } else { "" };
    let title = if app.replay_mode {
        "Event log".to_string()
    } else {
        format!("Event log · j/k active{follow_indicator}")
    };
    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    if app.events.is_empty() {
        let empty = Paragraph::new("No events")
            .block(block)
            .style(panel_style(surface, theme.text.secondary));
        frame.render_widget(empty, area);
        return;
    }

    let items: Vec<Line> = app
        .events
        .iter()
        .enumerate()
        .skip(app.events_trimmed_count)
        .map(|(idx, event)| {
            let display_idx = idx + 1;
            let is_selected = idx == app.selected_event_index;
            let prefix = if is_selected { ">" } else { " " };

            let style = if is_selected {
                Style::default()
                    .fg(theme.text.inverse)
                    .bg(theme.border.focus)
                    .add_modifier(Modifier::BOLD)
            } else {
                panel_style(surface, theme.text.primary)
            };

            let event_type = format!("{:?}", event.payload)
                .split(':')
                .next()
                .unwrap_or("Unknown")
                .to_string();

            let content = format!("{:>5} {} {}", display_idx, prefix, event_type);
            Line::from(Span::styled(content, style))
        })
        .collect();

    let list = Paragraph::new(Text::from(items))
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: false });

    frame.render_widget(list, area);
}

fn render_event_details(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let is_focused = app.focus == Focus::Details;

    let title = if app.replay_mode {
        "Selected event"
    } else {
        "Event details"
    };
    let surface = theme.surface.panel_elevated;
    let block = panel_block(theme, title, is_focused, surface);

    let content = if let Some(event) = app.selected_event() {
        match serde_json::to_string_pretty(event) {
            Ok(json) => json,
            Err(_) => "Error serializing event".to_string(),
        }
    } else {
        "No event selected".to_string()
    };

    let paragraph = Paragraph::new(content)
        .block(block)
        .style(panel_style(surface, theme.text.primary))
        .wrap(Wrap { trim: true });

    frame.render_widget(paragraph, area);
}

fn load_diff_for_event(
    session_path: &std::path::Path,
    event: &harness_core::event::EventEnvelopeV1,
) -> Option<String> {
    use harness_core::event::EventV1;

    if let EventV1::EditApplied(data) = &event.payload {
        if let Some(diff_rel_path) = &data.diff_rel_path {
            let diff_path = session_path.join(diff_rel_path);
            return std::fs::read_to_string(&diff_path).ok();
        }
    }
    None
}

fn render_secondary_surface_shell(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    summary: Line<'static>,
) -> Rect {
    let layout = secondary_surface_layout(area, theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.shell)),
        layout.shell,
    );
    if layout.body.width == 0 || layout.body.height == 0 {
        return layout.body;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(layout.body);
    frame.render_widget(
        Paragraph::new(summary).style(panel_style(theme.surface.shell, theme.text.secondary)),
        chunks[0],
    );
    chunks[1]
}

fn secondary_summary_line(
    app: &AppState,
    label: &'static str,
    accent: Color,
    detail: impl Into<String>,
    theme: &Theme,
) -> Line<'static> {
    Line::from(vec![
        status_badge(label, accent, theme),
        Span::styled("  ", panel_style(theme.surface.shell, theme.text.secondary)),
        Span::styled(
            detail.into(),
            panel_style(theme.surface.shell, theme.text.secondary),
        ),
        Span::styled(
            if app.replay_mode {
                "  ·  read-only"
            } else {
                ""
            },
            panel_style(theme.surface.shell, theme.text.tertiary),
        ),
    ])
}

fn events_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let selected = app.selected_event().map_or(0, |event| event.seq);
    secondary_summary_line(
        app,
        "events",
        theme.border.strong,
        format!(
            "{} recorded · selected seq {}{}",
            app.events.len(),
            selected,
            if app.follow_mode { " · follow on" } else { "" }
        ),
        theme,
    )
}

fn diff_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    let detail = if let Some(event) = app.selected_event() {
        format!("artifact view · seq {}", event.seq)
    } else {
        "artifact view · select an edit event".to_string()
    };
    secondary_summary_line(app, "diff", theme.status.info, detail, theme)
}

fn help_summary_line(app: &AppState, theme: &Theme) -> Line<'static> {
    secondary_summary_line(
        app,
        "help",
        theme.border.strong,
        if app.replay_mode {
            "replay controls and read-only navigation"
        } else {
            "live controls, drawers, and prompt shortcuts"
        },
        theme,
    )
}
