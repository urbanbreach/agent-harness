use super::*;

pub(super) fn render_overlays(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    plan: &FrameLayoutPlan,
) {
    for overlay in &app.overlay_stack() {
        match overlay {
            OverlayKind::DetailsDrawer => {}
            OverlayKind::CommandPalette => {
                render_command_palette_overlay(frame, app, theme, plan.palette_overlay)
            }
            OverlayKind::PermissionModal => {
                if let Some(permission) = app.active_permission_view() {
                    if let Some(modal) = plan.permission_modal {
                        render_permission_modal(frame, &permission, theme, modal);
                    }
                }
            }
        }
    }
}

fn render_command_palette_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    frame.render_widget(Clear, overlay);
    let card_surface = theme.surface.panel_elevated;
    let title = if app.session_history_visible {
        session_history_overlay_title(app)
    } else {
        "Command palette".to_string()
    };
    let block = elevated_card_block(
        Line::from(Span::styled(
            title,
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        )),
        card_surface,
        theme.border.focus,
        theme.text.accent,
    );
    let inner = block.inner(overlay);
    frame.render_widget(block, overlay);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    if app.session_history_visible {
        render_session_history_overlay(frame, app, theme, inner, card_surface);
    } else {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Min(0)])
            .split(inner);

        render_command_palette_input(frame, app, theme, sections[0]);
        render_command_palette_list(frame, app, theme, sections[1]);
    }
}

fn render_session_history_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    area: Rect,
    card_surface: Color,
) {
    let show_banner = app.continue_disabled_banner.is_some();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if show_banner { 1 } else { 0 }),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(area);

    if let Some(banner) = app.continue_disabled_banner.as_deref() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                truncate_plain_text(banner, usize::from(sections[0].width)),
                Style::default()
                    .fg(theme.status.warning)
                    .bg(card_surface)
                    .add_modifier(Modifier::BOLD),
            ))),
            sections[0],
        );
    }

    render_command_palette_input(frame, app, theme, sections[1]);
    frame.render_widget(
        Paragraph::new(session_history_scope_line(app))
            .style(Style::default().fg(theme.text.secondary).bg(card_surface)),
        sections[2],
    );
    render_session_history_list(frame, app, theme, sections[3]);
}

fn render_command_palette_input(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(theme.surface.overlay)),
        area,
    );

    let mut input = app.palette_input.clone();
    let cursor_byte = input
        .char_indices()
        .nth(app.palette_cursor)
        .map(|(index, _)| index)
        .unwrap_or(input.len());
    input.insert(cursor_byte, '█');

    let line = Line::from(vec![
        Span::styled(
            "> ",
            Style::default()
                .fg(theme.text.accent)
                .bg(theme.surface.overlay)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            input,
            Style::default()
                .fg(theme.text.primary)
                .bg(theme.surface.overlay),
        ),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn render_command_palette_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.palette_filtered.is_empty() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "No commands",
                Style::default().fg(theme.text.secondary),
            ))),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height);
    let selected = app
        .palette_selected
        .min(app.palette_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (row, command) in app
        .palette_filtered
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
        if is_selected {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.surface.overlay)),
                row_area,
            );
        }

        frame.render_widget(
            Paragraph::new(command_palette_row(
                Action::palette_command_label(command),
                palette_command_description(command),
                is_selected,
                theme,
                row_area.width,
            )),
            row_area,
        );
    }
}

fn command_palette_row(
    label: &str,
    description: &str,
    is_selected: bool,
    theme: &Theme,
    width: u16,
) -> Line<'static> {
    let row_width = usize::from(width);
    let row_style = if is_selected {
        Style::default()
            .fg(theme.text.inverse)
            .bg(theme.surface.overlay)
    } else {
        Style::default()
    };
    let label_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(theme.text.primary)
    };
    let description_style = if is_selected {
        row_style
    } else {
        row_style.fg(theme.text.secondary)
    };

    let mut spans = vec![Span::styled(label.to_string(), label_style)];
    let mut used_width = label.chars().count();

    let gap_width = 2;
    let available_description = row_width.saturating_sub(used_width.saturating_add(gap_width));
    let description = truncate_plain_text(description, available_description);
    if !description.is_empty() {
        spans.push(Span::styled("  ", row_style));
        used_width = used_width.saturating_add(gap_width);
        used_width = used_width.saturating_add(description.chars().count());
        spans.push(Span::styled(description, description_style));
    }

    if is_selected && used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

fn render_session_history_list(frame: &mut Frame, app: &AppState, theme: &Theme, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.session_history_filtered.is_empty() {
        let empty = if app.session_history_entries.is_empty() {
            "No saved runs yet — launch one and it will appear here."
        } else {
            "No saved runs match this filter."
        };
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                empty,
                Style::default().fg(theme.text.secondary),
            ))),
            area,
        );
        return;
    }

    let row_height = 1usize;
    let visible_rows = (usize::from(area.height) / row_height).max(1);
    let selected = app
        .session_history_selected
        .min(app.session_history_filtered.len().saturating_sub(1));
    let scroll = selected.saturating_sub(visible_rows.saturating_sub(1));

    for (visible_index, entry_index) in app
        .session_history_filtered
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_rows)
    {
        let entry = &app.session_history_entries[*entry_index];
        let row_offset = (visible_index - scroll) * row_height;
        let row_y = area
            .y
            .saturating_add(u16::try_from(row_offset).unwrap_or(u16::MAX));
        if row_y >= area.y.saturating_add(area.height) {
            break;
        }

        let remaining_height = area
            .height
            .saturating_sub(u16::try_from(row_offset).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, remaining_height.min(1));
        let is_selected = visible_index == selected;
        if is_selected {
            frame.render_widget(
                Block::default().style(Style::default().bg(theme.border.focus)),
                row_area,
            );
        }

        frame.render_widget(
            Paragraph::new(session_history_row(entry, app, is_selected, theme)),
            row_area,
        );
    }
}

fn session_history_overlay_title(app: &AppState) -> String {
    let total = app.session_history_filtered.len();
    let matches_label = format!("{total} match{}", if total == 1 { "" } else { "es" });
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => format!("Replay session · {matches_label}"),
        StartupLauncherAction::ContinueSession => {
            let blocked = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    !app.session_history_entries[**entry_index]
                        .catalog
                        .is_resumable
                })
                .count();
            if blocked > 0 {
                format!("Continue session · {matches_label} · {blocked} blocked")
            } else {
                format!("Continue session · {matches_label}")
            }
        }
        StartupLauncherAction::NewSession => format!("Session history · {matches_label}"),
    }
}

fn session_history_scope_line(app: &AppState) -> String {
    match app.startup_launcher_action {
        StartupLauncherAction::ContinueSession => {
            let ready = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    app.session_history_entries[**entry_index]
                        .catalog
                        .is_resumable
                })
                .count();
            let blocked = app.session_history_filtered.len().saturating_sub(ready);
            if app.session_history_filtered.is_empty() {
                "Interactive histories only · blocked rows stay visible when they match".to_string()
            } else if blocked == 0 {
                format!("Interactive histories · {ready} ready · filter by run/profile/model")
            } else {
                format!(
                    "Interactive histories · {ready} ready · {blocked} blocked · blocked rows stay visible"
                )
            }
        }
        StartupLauncherAction::ReplaySession => {
            let prompt_only = app
                .session_history_filtered
                .iter()
                .filter(|entry_index| {
                    matches!(
                        app.session_history_entries[**entry_index]
                            .catalog
                            .mode_source,
                        harness_core::proj::SessionModeSource::Prompt
                    )
                })
                .count();
            if prompt_only > 0 {
                format!(
                    "Read-only replays · {} matching · {prompt_only} prompt-only still visible",
                    app.session_history_filtered.len()
                )
            } else {
                format!(
                    "Read-only replays · {} matching · interactive and prompt runs stay available",
                    app.session_history_filtered.len()
                )
            }
        }
        StartupLauncherAction::NewSession => {
            "Browse saved runs without losing the draft in the launcher".to_string()
        }
    }
}

fn session_history_row(
    entry: &crate::app::SessionHistoryEntry,
    app: &AppState,
    is_selected: bool,
    theme: &Theme,
) -> Line<'static> {
    let row_style = if is_selected {
        Style::default()
            .fg(theme.text.inverse)
            .bg(theme.border.focus)
    } else {
        Style::default()
    };
    let title_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    };
    let meta_style = if is_selected {
        row_style
    } else {
        Style::default().fg(theme.text.secondary)
    };
    let action_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(session_history_action_color(app, entry, theme))
            .add_modifier(Modifier::BOLD)
    };
    let status_style = if is_selected {
        row_style
    } else {
        Style::default().fg(session_history_status_color(entry, theme))
    };
    let capability_style = if is_selected {
        row_style
    } else {
        Style::default().fg(match app.startup_launcher_action {
            StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
                if entry.catalog.is_resumable {
                    theme.status.success
                } else {
                    theme.status.warning
                }
            }
            StartupLauncherAction::ReplaySession => theme.status.info,
        })
    };

    let capability = session_history_capability_label(entry, app.startup_launcher_action);
    let source = format!(
        "{}/{}",
        session_history_profile_label(entry),
        session_history_provider_model_label(entry)
    );

    Line::from(vec![
        Span::styled(session_history_action_prefix(app, entry), action_style),
        Span::styled(session_history_run_name(entry).to_string(), title_style),
        Span::styled(" · ", meta_style),
        Span::styled(capability, capability_style),
        Span::styled(" · ", meta_style),
        Span::styled(
            session_history_status_label(entry).to_string(),
            status_style,
        ),
        Span::styled(" · ", meta_style),
        Span::styled(source, meta_style),
    ])
}

fn session_history_capability_label(
    entry: &crate::app::SessionHistoryEntry,
    action: StartupLauncherAction,
) -> String {
    match action {
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            session_history_resumability_label(entry)
        }
        StartupLauncherAction::ReplaySession => match entry.catalog.mode_source {
            harness_core::proj::SessionModeSource::Prompt => "prompt-only replay ready".to_string(),
            harness_core::proj::SessionModeSource::InteractiveLive
            | harness_core::proj::SessionModeSource::InteractiveMock => {
                if entry.catalog.is_resumable {
                    "replay ready · continue ready".to_string()
                } else {
                    entry
                        .catalog
                        .resume_disabled_reason
                        .as_deref()
                        .map(|reason| format!("replay ready · blocked: {reason}"))
                        .unwrap_or_else(|| "replay ready".to_string())
                }
            }
            harness_core::proj::SessionModeSource::ScenarioFixture => {
                "fixture replay ready".to_string()
            }
            harness_core::proj::SessionModeSource::ReplayOnly => {
                "replay artifact ready".to_string()
            }
            harness_core::proj::SessionModeSource::Unknown => "saved replay ready".to_string(),
        },
    }
}

fn session_history_action_prefix(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
) -> String {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => "↺ replay ".to_string(),
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                "▶ continue ".to_string()
            } else {
                "! blocked ".to_string()
            }
        }
    }
}

fn session_history_action_color(
    app: &AppState,
    entry: &crate::app::SessionHistoryEntry,
    theme: &Theme,
) -> Color {
    match app.startup_launcher_action {
        StartupLauncherAction::ReplaySession => theme.status.info,
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                theme.status.success
            } else {
                theme.status.warning
            }
        }
    }
}

fn session_history_status_color(entry: &crate::app::SessionHistoryEntry, theme: &Theme) -> Color {
    match entry.catalog.status {
        Some(harness_core::proj::RunStatus::Running) => theme.status.info,
        Some(harness_core::proj::RunStatus::Finished) => theme.status.success,
        Some(harness_core::proj::RunStatus::Failed) => theme.status.error,
        None => theme.text.secondary,
    }
}

fn palette_command_description(command: &str) -> &'static str {
    Action::palette_commands()
        .iter()
        .find_map(|(candidate, description)| (*candidate == command).then_some(*description))
        .unwrap_or("")
}

fn render_permission_modal(
    frame: &mut Frame,
    permission: &crate::app::ActivePermissionView,
    theme: &Theme,
    popup_rect: Rect,
) {
    frame.render_widget(Clear, popup_rect);
    let surface = theme.surface.overlay;
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let summary_style = Style::default().fg(theme.text.primary).bg(surface);
    let block = elevated_card_block(
        Line::from(vec![
            Span::styled(
                format!("{} ", theme.live_shell.glyphs.pending_permission),
                Style::default().fg(theme.status.warning),
            ),
            Span::styled(
                "Permission Requested",
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        surface,
        theme.status.warning,
        theme.text.accent,
    );
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            status_badge("FAIL CLOSED", theme.status.error, theme),
            Span::styled("  ", metadata_style),
            status_badge(
                permission.kind.replace('_', " ").to_uppercase(),
                theme.status.warning,
                theme,
            ),
        ]))
        .style(metadata_style),
        sections[0],
    );

    frame.render_widget(
        Paragraph::new(Text::from(vec![
            Line::from(vec![Span::styled(
                truncate_plain_text(&permission.summary, usize::from(sections[1].width)),
                summary_style.add_modifier(Modifier::BOLD),
            )]),
            Line::from(vec![Span::styled(
                format!(
                    "default {} · timeout {}s",
                    permission_default_label(permission.default_decision),
                    permission.timeout_ms / 1_000
                ),
                metadata_style,
            )]),
            Line::from(vec![Span::styled(
                truncate_plain_text(
                    &permission_identity_line(permission),
                    usize::from(sections[1].width),
                ),
                metadata_style,
            )]),
            Line::from(vec![Span::styled(
                truncate_plain_text(
                    "Review the request before granting access. Deny keeps the run fail-closed.",
                    usize::from(sections[1].width),
                ),
                metadata_style,
            )]),
        ]))
        .style(summary_style)
        .wrap(Wrap { trim: true }),
        sections[1],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            status_badge("[d] deny", theme.status.error, theme),
            Span::styled("  ", metadata_style),
            Span::styled("[esc] dismiss", metadata_style),
            Span::styled("  ", metadata_style),
            status_badge("[a] allow once", theme.border.focus, theme),
        ]))
        .style(
            Style::default()
                .fg(theme.text.secondary)
                .bg(theme.surface.panel_elevated)
                .add_modifier(Modifier::BOLD),
        )
        .alignment(Alignment::Center),
        sections[2],
    );
}

fn permission_default_label(decision: harness_core::event::PermissionDecision) -> &'static str {
    match decision {
        harness_core::event::PermissionDecision::Allow => "allow",
        harness_core::event::PermissionDecision::Deny => "deny",
    }
}

fn permission_identity_line(permission: &crate::app::ActivePermissionView) -> String {
    let tool = permission
        .tool_label
        .as_deref()
        .map(|tool| format!("tool {tool}"))
        .or_else(|| {
            permission
                .tool_call_id
                .as_deref()
                .map(|tool_call_id| format!("tool call {tool_call_id}"))
        })
        .unwrap_or_else(|| format!("permission {}", permission.permission_id));
    format!("{tool} · digest {}", permission.request_digest)
}
