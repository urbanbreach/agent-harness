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
                render_command_palette_overlay(frame, app, theme, plan.root, plan.palette_overlay)
            }
            OverlayKind::PermissionModal => {
                if let Some(permission) = app.active_permission_view() {
                    if let Some(modal) = plan.permission_modal {
                        render_permission_modal(frame, app, &permission, theme, plan.root, modal);
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
    root: Rect,
    overlay: Option<Rect>,
) {
    let Some(overlay) = overlay else {
        return;
    };

    render_overlay_backdrop(frame, root, ui_chrome::quiet_modal_backdrop_surface(theme));

    let title = if app.session_history_visible {
        session_history_overlay_title(app)
    } else {
        "Command palette".to_string()
    };
    let Some(inner) = render_overlay_surface(frame, theme, overlay, &title) else {
        return;
    };

    let card_surface = ui_chrome::elevated_card_surface(theme);

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
    let area = inset_rect(area, 1, 0);
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
                truncate_plain_text(
                    &overlay_continue_banner_text(banner),
                    usize::from(sections[0].width),
                ),
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

    let surface = theme.surface.overlay;

    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

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
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(input, Style::default().fg(theme.text.primary).bg(surface)),
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
    let rows = palette_overlay_rows(app);
    let selected_row = rows
        .iter()
        .position(|row| matches!(row, PaletteOverlayRow::Command { command, .. } if *command == app.palette_filtered[selected]))
        .unwrap_or(0);
    let scroll = selected_row.saturating_sub(visible_rows.saturating_sub(1));

    for (row, palette_row) in rows.iter().enumerate().skip(scroll).take(visible_rows) {
        let row_y = area
            .y
            .saturating_add(u16::try_from(row - scroll).unwrap_or(u16::MAX));
        let row_area = Rect::new(area.x, row_y, area.width, 1);
        match palette_row {
            PaletteOverlayRow::Section(section) => {
                frame.render_widget(
                    Paragraph::new(Line::from(Span::styled(
                        section.label(),
                        Style::default()
                            .fg(theme.text.accent)
                            .bg(theme.surface.overlay)
                            .add_modifier(Modifier::BOLD),
                    ))),
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

enum PaletteOverlayRow<'a> {
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
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
    } else {
        Style::default()
    };
    let prefix_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.secondary)
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
    let shortcut_style = if is_selected {
        row_style.add_modifier(Modifier::BOLD)
    } else {
        row_style.fg(theme.text.secondary)
    };

    let prefix = if is_selected { "› " } else { "  " };
    let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
    let mut used_width = prefix.chars().count();
    let reserved_shortcut = if shortcut.is_empty() {
        0
    } else {
        shortcut.chars().count().saturating_add(2)
    };
    let body_width = row_width.saturating_sub(reserved_shortcut);

    let label = truncate_plain_text(label, body_width.saturating_sub(used_width));
    used_width = used_width.saturating_add(label.chars().count());
    spans.push(Span::styled(label, label_style));

    let gap_width = 2;
    let available_description = body_width.saturating_sub(used_width.saturating_add(gap_width));
    let description = truncate_plain_text(description, available_description);
    if !description.is_empty() {
        spans.push(Span::styled("  ", row_style));
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
    } else if is_selected && body_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - body_width), row_style));
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
                Block::default().style(ui_chrome::overlay_focus_row_style(theme)),
                row_area,
            );
        }

        frame.render_widget(
            Paragraph::new(session_history_row(
                entry,
                app,
                is_selected,
                theme,
                row_area.width,
            )),
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
                format!(
                    "Interactive histories · {ready} ready · filter by run/profile/model/lineage"
                )
            } else {
                format!(
                    "Interactive histories · {ready} ready · {blocked} blocked · filter by run/profile/model/lineage"
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
    width: u16,
) -> Line<'static> {
    let row_style = if is_selected {
        ui_chrome::overlay_focus_row_style(theme)
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

    let capability = overlay_session_history_capability_label(entry, app.startup_launcher_action);
    let artifact_label = session_history_artifact_label(entry);
    let lineage_label = session_history_lineage_label(entry);
    let source = format!(
        "{}/{}",
        session_history_profile_label(entry),
        session_history_provider_model_label(entry)
    );

    let row_width = usize::from(width);
    let prefix = session_history_action_prefix(app, entry);
    let reserved_capability_width = usize::from(row_width > 32) * 18;
    let title_budget = row_width
        .saturating_sub(
            prefix
                .chars()
                .count()
                .saturating_add(reserved_capability_width),
        )
        .max(12)
        .min(row_width.saturating_sub(prefix.chars().count()));
    let title = truncate_plain_text(session_history_run_name(entry), title_budget);

    let mut spans = vec![
        Span::styled(prefix.clone(), action_style),
        Span::styled(title.clone(), title_style),
    ];
    let mut used_width = prefix.chars().count().saturating_add(title.chars().count());

    append_session_history_segment(
        &mut spans,
        &mut used_width,
        row_width,
        &capability,
        meta_style,
        capability_style,
        8,
    );

    if row_width >= 58 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &artifact_label,
            meta_style,
            meta_style,
            8,
        );
    }

    if row_width >= 76 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &lineage_label,
            meta_style,
            meta_style,
            10,
        );
    }

    if row_width >= 92 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            session_history_status_label(entry),
            meta_style,
            status_style,
            6,
        );
    }

    if row_width >= 112 {
        append_session_history_segment(
            &mut spans,
            &mut used_width,
            row_width,
            &source,
            meta_style,
            meta_style,
            10,
        );
    }

    if is_selected && used_width < row_width {
        spans.push(Span::styled(" ".repeat(row_width - used_width), row_style));
    }

    Line::from(spans)
}

fn append_session_history_segment(
    spans: &mut Vec<Span<'static>>,
    used_width: &mut usize,
    row_width: usize,
    text: &str,
    separator_style: Style,
    text_style: Style,
    min_text_width: usize,
) {
    const SEPARATOR: &str = " · ";

    let separator_width = SEPARATOR.chars().count();
    let remaining = row_width.saturating_sub(*used_width);
    if remaining <= separator_width.saturating_add(min_text_width) {
        return;
    }

    let text = truncate_plain_text(text, remaining.saturating_sub(separator_width));
    if text.is_empty() {
        return;
    }

    spans.push(Span::styled(SEPARATOR.to_string(), separator_style));
    spans.push(Span::styled(text.clone(), text_style));
    *used_width = used_width
        .saturating_add(separator_width)
        .saturating_add(text.chars().count());
}

fn overlay_continue_banner_text(banner: &str) -> String {
    banner
        .strip_prefix("continue unavailable: ")
        .map(|reason| format!("blocked · {reason}"))
        .unwrap_or_else(|| banner.to_string())
}

fn overlay_session_history_capability_label(
    entry: &crate::app::SessionHistoryEntry,
    action: StartupLauncherAction,
) -> String {
    match action {
        StartupLauncherAction::ContinueSession | StartupLauncherAction::NewSession => {
            if entry.catalog.is_resumable {
                "continue ready".to_string()
            } else {
                entry
                    .catalog
                    .resume_disabled_reason
                    .as_deref()
                    .map(|reason| format!("blocked · {reason}"))
                    .unwrap_or_else(|| "blocked".to_string())
            }
        }
        StartupLauncherAction::ReplaySession => session_history_capability_label(entry, action),
    }
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
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    theme: &Theme,
    root: Rect,
    popup_rect: Rect,
) {
    render_overlay_backdrop(frame, root, theme.surface.canvas);
    frame.render_widget(Clear, popup_rect);
    let surface = ui_chrome::elevated_card_surface(theme);
    let title = permission_modal_title(permission);
    let submission_pending = app.permission_submission_pending(&permission.permission_id);
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let summary_style = Style::default().fg(theme.text.primary).bg(surface);
    let guidance_style = Style::default().fg(theme.text.primary).bg(surface);
    let block = ui_chrome::interruptive_modal_block(
        theme,
        Line::from(vec![
            Span::styled(
                format!("{} ", theme.live_shell.glyphs.pending_permission),
                Style::default().fg(theme.status.warning),
            ),
            Span::styled(
                title,
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        theme.status.warning,
        theme.text.accent,
        ui_chrome::ChromeFrame::Frame,
    );
    let inner = block.inner(popup_rect);
    frame.render_widget(block, popup_rect);

    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let is_question = permission.question_prompts.is_some();
    let sections = if is_question {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(6),
                Constraint::Length(5),
                Constraint::Length(2),
            ])
            .split(inner)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(4),
                Constraint::Length(2),
            ])
            .split(inner)
    };

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            status_badge("FAIL CLOSED", theme.status.error, theme),
            Span::styled("  ", metadata_style),
            Span::styled(
                "SESSION PAUSED",
                Style::default()
                    .fg(theme.status.warning)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ),
        ]))
        .style(metadata_style),
        sections[0],
    );

    if let Some(prompts) = permission.question_prompts.as_ref() {
        frame.render_widget(
            Paragraph::new(question_permission_body_text(
                permission,
                prompts,
                submission_pending,
                app.prompt_buffer.as_str(),
                metadata_style,
                summary_style,
                guidance_style,
            ))
            .style(summary_style)
            .wrap(Wrap { trim: true }),
            sections[1],
        );
        frame.render_widget(
            Paragraph::new(question_permission_answer_text(
                app, permission, theme, surface,
            ))
            .style(Style::default().fg(theme.text.primary).bg(surface))
            .wrap(Wrap { trim: false }),
            sections[2],
        );
    } else {
        frame.render_widget(
            Paragraph::new(Text::from(vec![
                Line::from(vec![Span::styled(
                    truncate_plain_text(
                        &permission_modal_summary_line(permission, submission_pending),
                        usize::from(sections[1].width),
                    ),
                    summary_style.add_modifier(Modifier::BOLD),
                )]),
                Line::from(vec![Span::styled(
                    truncate_plain_text(
                        permission_modal_guidance(permission, submission_pending),
                        usize::from(sections[1].width),
                    ),
                    guidance_style,
                )]),
                Line::from(vec![Span::styled(
                    truncate_plain_text(
                        &permission_modal_draft_line(app.prompt_buffer.as_str()),
                        usize::from(sections[1].width),
                    ),
                    metadata_style,
                )]),
                Line::from(vec![Span::styled(
                    truncate_plain_text(
                        &permission_modal_metadata_line(permission),
                        usize::from(sections[1].width),
                    ),
                    metadata_style,
                )]),
            ]))
            .style(summary_style)
            .wrap(Wrap { trim: true }),
            sections[1],
        );
    }

    frame.render_widget(
        Paragraph::new(permission_modal_actions_text(
            app,
            theme,
            surface,
            submission_pending,
            is_question,
        ))
        .style(Style::default().fg(theme.text.secondary).bg(surface))
        .wrap(Wrap { trim: true }),
        sections[if is_question { 3 } else { 2 }],
    );
}

fn permission_modal_metadata_line(permission: &crate::app::ActivePermissionView) -> String {
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

fn render_overlay_surface(
    frame: &mut Frame,
    theme: &Theme,
    overlay: Rect,
    title: &str,
) -> Option<Rect> {
    if overlay.width == 0 || overlay.height == 0 {
        return None;
    }

    let block = ui_chrome::elevated_card_block(
        Line::from(Span::styled(
            title.to_string(),
            Style::default()
                .fg(theme.text.accent)
                .add_modifier(Modifier::BOLD),
        )),
        ui_chrome::elevated_card_surface(theme),
        theme.border.focus,
        theme.text.accent,
    );
    let content = inset_rect(block.inner(overlay), 1, 0);

    frame.render_widget(Clear, overlay);
    frame.render_widget(block, overlay);

    if content.width == 0 || content.height == 0 {
        return None;
    }

    Some(content)
}

fn permission_modal_title(permission: &crate::app::ActivePermissionView) -> &'static str {
    if permission.kind.eq_ignore_ascii_case("question")
        || permission.kind.eq_ignore_ascii_case("ask")
        || permission.kind.eq_ignore_ascii_case("ask_user")
    {
        "Question Requested"
    } else {
        "Permission Requested"
    }
}

fn permission_modal_guidance(
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

fn permission_modal_summary_line(
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

fn permission_modal_draft_line(prompt_buffer: &str) -> String {
    let draft = prompt_buffer.trim();
    if draft.is_empty() {
        "Draft preserved beneath this checkpoint.".to_string()
    } else {
        format!("Draft preserved · {draft}")
    }
}

fn render_overlay_backdrop(frame: &mut Frame, area: Rect, background: Color) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(background)),
        area,
    );
}

fn permission_modal_actions_text(
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
    let later_label = app.keymap.get_binding_label(Action::DismissModal, "later");
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
                    format!("{later_label} defers the question")
                } else {
                    format!("{later_label} keeps draft")
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

fn question_permission_body_text(
    permission: &crate::app::ActivePermissionView,
    prompts: &[crate::app::QuestionPromptView],
    submission_pending: bool,
    prompt_buffer: &str,
    metadata_style: Style,
    summary_style: Style,
    guidance_style: Style,
) -> Text<'static> {
    let mut lines = vec![
        Line::from(vec![Span::styled(
            permission_modal_summary_line(permission, submission_pending),
            summary_style.add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            permission_modal_guidance(permission, submission_pending),
            guidance_style,
        )]),
        Line::from(vec![Span::styled(
            permission_modal_draft_line(prompt_buffer),
            metadata_style,
        )]),
        Line::from(vec![Span::styled(
            permission_modal_metadata_line(permission),
            metadata_style,
        )]),
        Line::from(""),
    ];

    for (index, prompt) in prompts.iter().enumerate() {
        lines.push(Line::from(vec![Span::styled(
            format!("[{}] {}", prompt.header, prompt.question),
            summary_style.add_modifier(Modifier::BOLD),
        )]));
        for option in &prompt.options {
            lines.push(Line::from(vec![Span::styled(
                format!("  - {} — {}", option.label, option.description),
                guidance_style,
            )]));
        }
        lines.push(Line::from(vec![Span::styled(
            if prompt.multiple {
                format!(
                    "  answer line {} with comma-separated labels or custom text",
                    index + 1
                )
            } else {
                format!("  answer line {} with one label or custom text", index + 1)
            },
            metadata_style,
        )]));
        lines.push(Line::from(""));
    }

    Text::from(lines)
}

fn question_permission_answer_text(
    app: &AppState,
    permission: &crate::app::ActivePermissionView,
    theme: &Theme,
    surface: Color,
) -> Text<'static> {
    let metadata_style = Style::default().fg(theme.text.secondary).bg(surface);
    let primary_style = Style::default().fg(theme.text.primary).bg(surface);
    let mut lines = vec![
        Line::from(vec![Span::styled(
            "Answers (one line per question)",
            primary_style.add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            app.question_answer_preview(&permission.permission_id),
            primary_style,
        )]),
    ];
    if let Some(error) = app.question_answer_error(&permission.permission_id) {
        lines.push(Line::from(vec![Span::styled(
            error.to_string(),
            Style::default().fg(theme.status.error).bg(surface),
        )]));
    } else {
        lines.push(Line::from(vec![Span::styled(
            "Press Enter for a new line, then send with allow once.",
            metadata_style,
        )]));
    }
    Text::from(lines)
}
