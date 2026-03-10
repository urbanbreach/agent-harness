use super::*;

#[derive(Debug, Default, Clone, Copy)]
struct StartupHistoryStats {
    continue_ready: usize,
    continue_blocked: usize,
    replay_total: usize,
    replay_prompt_only: usize,
}

pub(super) fn live_empty_state_visible(app: &AppState) -> bool {
    !app.replay_mode
        && !app.startup_shell_visible()
        && app.activities.is_empty()
        && app.transcript_pending_permissions().is_empty()
        && app.prompt_buffer.is_empty()
}

pub(super) fn startup_shell_visible(app: &AppState) -> bool {
    app.startup_shell_visible()
}

fn startup_action_style(
    app: &AppState,
    theme: &Theme,
    row_surface: Color,
    action: StartupLauncherAction,
) -> Style {
    let is_selected = app.startup_launcher_action == action;
    let list_focused = app.focus == Focus::List;

    if is_selected && list_focused {
        Style::default()
            .fg(theme.text.inverse)
            .bg(row_surface)
            .add_modifier(Modifier::BOLD)
    } else if is_selected {
        Style::default()
            .fg(theme.text.primary)
            .bg(row_surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.secondary).bg(row_surface)
    }
}

fn startup_action_color(theme: &Theme, action: StartupLauncherAction) -> Color {
    match action {
        StartupLauncherAction::NewSession => theme.text.accent,
        StartupLauncherAction::ContinueSession => theme.status.success,
        StartupLauncherAction::ReplaySession => theme.status.info,
    }
}

fn startup_action_icon(action: StartupLauncherAction) -> &'static str {
    match action {
        StartupLauncherAction::NewSession => "+",
        StartupLauncherAction::ContinueSession => "▶",
        StartupLauncherAction::ReplaySession => "↺",
    }
}

fn startup_history_stats(app: &AppState) -> StartupHistoryStats {
    let mut stats = StartupHistoryStats::default();

    for entry in &app.session_history_entries {
        match entry.catalog.mode_source {
            harness_core::proj::SessionModeSource::InteractiveLive
            | harness_core::proj::SessionModeSource::InteractiveMock => {
                if entry.catalog.is_resumable {
                    stats.continue_ready += 1;
                } else {
                    stats.continue_blocked += 1;
                }
                stats.replay_total += 1;
            }
            harness_core::proj::SessionModeSource::Prompt
            | harness_core::proj::SessionModeSource::Unknown => {
                stats.replay_total += 1;
                if matches!(
                    entry.catalog.mode_source,
                    harness_core::proj::SessionModeSource::Prompt
                ) {
                    stats.replay_prompt_only += 1;
                }
            }
            harness_core::proj::SessionModeSource::ScenarioFixture
            | harness_core::proj::SessionModeSource::ReplayOnly => {}
        }
    }

    stats
}

fn startup_count_label(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

fn startup_action_purpose(app: &AppState, theme: &Theme, action: StartupLauncherAction) -> String {
    let stats = startup_history_stats(app);
    let copy = theme.live_shell.startup;
    match action {
        StartupLauncherAction::NewSession => copy.new_session_purpose.to_string(),
        StartupLauncherAction::ContinueSession => {
            let total = stats.continue_ready + stats.continue_blocked;
            if total == 0 {
                format!(
                    "{} · no interactive runs yet",
                    copy.continue_session_purpose
                )
            } else {
                let mut segments = vec![
                    copy.continue_session_purpose.to_string(),
                    format!("{} ready", stats.continue_ready),
                ];
                if stats.continue_blocked > 0 {
                    segments.push(format!("{} blocked", stats.continue_blocked));
                }
                segments.join(" · ")
            }
        }
        StartupLauncherAction::ReplaySession => {
            if stats.replay_total == 0 {
                format!("{} · no saved runs yet", copy.replay_session_purpose)
            } else {
                let mut segments = vec![
                    copy.replay_session_purpose.to_string(),
                    format!("{} available", stats.replay_total),
                ];
                if stats.replay_prompt_only > 0 {
                    segments.push(format!(
                        "{} prompt-only",
                        startup_count_label(stats.replay_prompt_only, "run")
                    ));
                }
                segments.join(" · ")
            }
        }
    }
}

fn startup_history_summary(app: &AppState) -> String {
    let stats = startup_history_stats(app);

    if stats.replay_total == 0 {
        return "No saved runs yet · type below to quick-start the first harness session"
            .to_string();
    }

    let mut segments = Vec::new();
    if stats.continue_blocked > 0 {
        segments.push(format!(
            "{} stay visible with reasons",
            startup_count_label(stats.continue_blocked, "blocked run")
        ));
    }
    if stats.replay_prompt_only > 0 {
        segments.push(format!(
            "{} stay replayable",
            startup_count_label(stats.replay_prompt_only, "prompt-only run")
        ));
    }
    if segments.is_empty() {
        segments.push(format!(
            "{} ready across Continue and Replay",
            startup_count_label(stats.replay_total, "saved run")
        ));
    }
    segments.join(" · ")
}

fn startup_action_line(
    app: &AppState,
    theme: &Theme,
    action: StartupLauncherAction,
    width: u16,
    row_surface: Color,
) -> Line<'static> {
    let row_style = startup_action_style(app, theme, row_surface, action);
    let is_selected = app.startup_launcher_action == action;
    let action_color = startup_action_color(theme, action);
    let label = format!("{} {}", startup_action_icon(action), action.label());
    let detail = startup_action_purpose(app, theme, action);
    let label_style = if is_selected {
        row_style
    } else {
        row_style.fg(action_color).add_modifier(Modifier::BOLD)
    };
    let detail_style = if is_selected {
        row_style
    } else {
        row_style.fg(theme.text.secondary)
    };
    let available_detail_width = usize::from(width)
        .saturating_sub(label.chars().count())
        .saturating_sub(3);

    Line::from(vec![
        Span::styled(label, label_style),
        Span::styled(" · ", detail_style),
        Span::styled(
            truncate_plain_text(&detail, available_detail_width),
            detail_style,
        ),
    ])
}

fn startup_action_row(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    surface: Color,
    area: Rect,
    action: StartupLauncherAction,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let row_surface = if app.startup_launcher_action == action {
        if app.focus == Focus::List {
            theme.border.focus
        } else {
            theme.surface.panel_elevated
        }
    } else {
        surface
    };
    let row_area = inset_rect(area, 1, 0);
    frame.render_widget(
        Block::default().style(Style::default().bg(row_surface)),
        row_area,
    );
    frame.render_widget(
        Paragraph::new(startup_action_line(
            app,
            theme,
            action,
            row_area.width,
            row_surface,
        ))
        .style(Style::default().bg(row_surface)),
        row_area,
    );
}

fn post_run_action_style(
    app: &AppState,
    theme: &Theme,
    row_surface: Color,
    action: PostRunHandoffAction,
) -> Style {
    let is_selected = app.selected_post_run_handoff_action() == action;
    let list_focused = app.focus == Focus::List;

    if is_selected && list_focused {
        Style::default()
            .fg(theme.text.inverse)
            .bg(row_surface)
            .add_modifier(Modifier::BOLD)
    } else if is_selected {
        Style::default()
            .fg(theme.text.primary)
            .bg(row_surface)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme.text.secondary).bg(row_surface)
    }
}

fn post_run_action_color(theme: &Theme, action: PostRunHandoffAction) -> Color {
    match action {
        PostRunHandoffAction::ContinueSession => theme.status.success,
        PostRunHandoffAction::ReplayRun => theme.status.info,
        PostRunHandoffAction::StartAnotherSession => theme.text.accent,
        PostRunHandoffAction::Quit => theme.text.secondary,
    }
}

fn post_run_action_icon(action: PostRunHandoffAction) -> &'static str {
    match action {
        PostRunHandoffAction::ContinueSession => "▶",
        PostRunHandoffAction::ReplayRun => "↺",
        PostRunHandoffAction::StartAnotherSession => "+",
        PostRunHandoffAction::Quit => "×",
    }
}

fn post_run_action_purpose(app: &AppState, action: PostRunHandoffAction) -> &'static str {
    match action {
        PostRunHandoffAction::ContinueSession => {
            if app.continued_post_run_handoff_active() {
                "resume this same run live from the composer"
            } else {
                "resume this run live from the composer"
            }
        }
        PostRunHandoffAction::ReplayRun => "inspect the run read-only",
        PostRunHandoffAction::StartAnotherSession => "launch a fresh session",
        PostRunHandoffAction::Quit => "close the TUI",
    }
}

fn post_run_action_line(
    app: &AppState,
    theme: &Theme,
    action: PostRunHandoffAction,
    width: u16,
    row_surface: Color,
) -> Line<'static> {
    let row_style = post_run_action_style(app, theme, row_surface, action);
    let is_selected = app.selected_post_run_handoff_action() == action;
    let label = format!(
        "{}{} {}",
        if is_selected { "› " } else { "  " },
        post_run_action_icon(action),
        action.label()
    );
    let detail = post_run_action_purpose(app, action);
    let label_style = if is_selected {
        row_style
    } else {
        row_style
            .fg(post_run_action_color(theme, action))
            .add_modifier(Modifier::BOLD)
    };
    let detail_style = if is_selected {
        row_style
    } else {
        row_style.fg(theme.text.secondary)
    };
    let available_detail_width = usize::from(width)
        .saturating_sub(label.chars().count())
        .saturating_sub(3);

    Line::from(vec![
        Span::styled(label, label_style),
        Span::styled(" · ", detail_style),
        Span::styled(
            truncate_plain_text(detail, available_detail_width),
            detail_style,
        ),
    ])
}

fn post_run_badge(app: &AppState, theme: &Theme) -> Span<'static> {
    if app.post_run_handoff_notice().is_some() {
        status_badge("Recovery only", theme.status.warning, theme)
    } else if matches!(app.runtime_state().kind, RuntimeStateKind::Failure) {
        status_badge("Recovery available", theme.status.warning, theme)
    } else {
        status_badge("Continue available", theme.status.success, theme)
    }
}

fn post_run_evidence_line(app: &AppState) -> &'static str {
    if app.post_run_handoff_notice().is_some() {
        "No reopen target is available · start fresh or quit safely"
    } else if matches!(app.runtime_state().kind, RuntimeStateKind::Failure) {
        "Continue resumes live for recovery · Replay stays read-only"
    } else {
        "Continue resumes live · Replay stays read-only"
    }
}

pub(super) fn render_continued_live_reopen_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    let lifecycle = theme.lifecycle_surface_layout(area.width, area.height);
    let shell_area = lifecycle_card_area(area, theme, lifecycle.post_run_card);
    let surface = theme.surface.panel;
    let title = Line::from(vec![
        Span::styled(
            "▶ ",
            Style::default()
                .fg(theme.status.success)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Continued live run",
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
    ]);
    let block = elevated_card_block(title, surface, theme.border.strong, theme.text.primary);
    let content_area = block.inner(shell_area);

    frame.render_widget(Clear, shell_area);
    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    let metadata = format!(
        "run {} · {}/{}",
        app.run_id().unwrap_or("unknown"),
        app.active_profile(),
        app.current_model_label()
    );
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            status_badge("Continued", theme.status.success, theme),
            Span::styled("  ", Style::default().bg(surface)),
            Span::styled(
                metadata,
                Style::default().fg(theme.text.tertiary).bg(surface),
            ),
        ]))
        .style(Style::default().bg(surface))
        .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new("Same run reopened live — continue from the composer below.")
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(
            "Use the existing run context, or inspect Events / Diff / Help before the next turn.",
        )
        .style(Style::default().fg(theme.text.secondary).bg(surface))
        .alignment(Alignment::Center),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new("Prompt submission stays live here — nothing is replay-only anymore.")
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new("").style(Style::default().fg(theme.text.tertiary).bg(surface)),
        rows[4],
    );
    frame.render_widget(
        Paragraph::new("continued live run")
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[5],
    );
}

fn post_run_action_row(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    surface: Color,
    area: Rect,
    action: PostRunHandoffAction,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let row_surface = if app.selected_post_run_handoff_action() == action {
        if app.focus == Focus::List {
            theme.border.focus
        } else {
            theme.surface.panel_elevated
        }
    } else {
        surface
    };
    let row_area = inset_rect(area, 1, 0);
    frame.render_widget(
        Block::default().style(Style::default().bg(row_surface)),
        row_area,
    );
    frame.render_widget(
        Paragraph::new(post_run_action_line(
            app,
            theme,
            action,
            row_area.width,
            row_surface,
        ))
        .style(Style::default().bg(row_surface)),
        row_area,
    );
}

pub(super) fn render_startup_lifecycle_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let shell_area = startup_shell_area(area, theme);
    let surface = theme.surface.panel;
    let list_focused = app.focus == Focus::List;
    let block = elevated_card_block(
        Line::from(Span::styled(
            theme.live_shell.startup.title,
            Style::default()
                .fg(if list_focused {
                    theme.text.accent
                } else {
                    theme.text.primary
                })
                .add_modifier(Modifier::BOLD),
        )),
        surface,
        if list_focused {
            theme.border.focus
        } else {
            theme.border.strong
        },
        if list_focused {
            theme.text.accent
        } else {
            theme.text.primary
        },
    );
    let content_area = block.inner(shell_area);

    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    let startup_card = app.startup_card_view_model();

    frame.render_widget(
        Paragraph::new(startup_card.metadata)
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(theme.live_shell.startup.subtitle)
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[1],
    );

    startup_action_row(
        frame,
        app,
        theme,
        surface,
        rows[2],
        StartupLauncherAction::ORDERED[0],
    );
    startup_action_row(
        frame,
        app,
        theme,
        surface,
        rows[3],
        StartupLauncherAction::ORDERED[1],
    );
    startup_action_row(
        frame,
        app,
        theme,
        surface,
        rows[4],
        StartupLauncherAction::ORDERED[2],
    );

    frame.render_widget(
        Paragraph::new(startup_history_summary(app))
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[5],
    );
    frame.render_widget(
        Paragraph::new(theme.live_shell.startup.secondary_hint)
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[6],
    );
}

pub(super) fn render_post_run_handoff_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let lifecycle = theme.lifecycle_surface_layout(area.width, area.height);
    let shell_area = lifecycle_card_area(area, theme, lifecycle.post_run_card);
    let surface = theme.surface.panel;
    let list_focused = app.focus == Focus::List;
    let block = elevated_card_block(
        Line::from(Span::styled(
            "Next action",
            Style::default()
                .fg(if list_focused {
                    theme.text.accent
                } else {
                    theme.text.primary
                })
                .add_modifier(Modifier::BOLD),
        )),
        surface,
        if list_focused {
            theme.border.focus
        } else {
            theme.border.strong
        },
        if list_focused {
            theme.text.accent
        } else {
            theme.text.primary
        },
    );
    let content_area = block.inner(shell_area);

    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    let card = app.post_run_card_view_model();
    frame.render_widget(
        Paragraph::new(Line::from(post_run_badge(app, theme)))
            .style(Style::default().bg(surface))
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(card.summary)
            .style(
                Style::default()
                    .fg(if card.warning {
                        theme.status.warning
                    } else {
                        theme.text.primary
                    })
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(post_run_evidence_line(app))
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[2],
    );

    for (row, action) in rows[3..7]
        .iter()
        .copied()
        .zip(app.post_run_handoff_actions().iter().copied())
    {
        post_run_action_row(frame, app, theme, surface, row, action);
    }

    frame.render_widget(
        Paragraph::new(if app.post_run_handoff_notice().is_some() {
            "Reopen stays blocked until a valid session target exists."
        } else {
            "Action order stays fixed: Continue → Replay → Start another → Quit"
        })
        .style(Style::default().fg(theme.text.tertiary).bg(surface))
        .alignment(Alignment::Center),
        rows[7],
    );
}

pub(super) fn render_live_empty_state(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    if app.continued_live_reopen_surface_visible() {
        render_continued_live_reopen_surface(frame, app, area, theme);
        return;
    }

    let help_row = [
        app.keymap.get_binding_label(Action::SubmitPrompt, "send"),
        app.keymap
            .get_binding_label(Action::InsertNewline, "newline"),
        format!(
            "{}/{} history",
            app.keymap.get_binding_str(Action::HistoryUp),
            app.keymap.get_binding_str(Action::HistoryDown)
        ),
    ]
    .join(" · ");

    let shell_area = live_empty_state_area(area, theme);
    let surface = theme.surface.panel;
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border.strong))
        .style(Style::default().bg(surface));
    let content_area = block.inner(shell_area);

    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(content_area);

    frame.render_widget(
        Paragraph::new(theme.live_shell.empty_state.title)
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );
    let startup_card = app.startup_card_view_model();
    frame.render_widget(
        Paragraph::new(startup_card.metadata)
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new("").style(Style::default().fg(theme.text.tertiary).bg(surface)),
        rows[2],
    );
    frame.render_widget(
        Paragraph::new(theme.live_shell.empty_state.value_prop)
            .style(
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[3],
    );
    frame.render_widget(
        Paragraph::new(help_row)
            .style(Style::default().fg(theme.text.secondary).bg(surface))
            .alignment(Alignment::Center),
        rows[4],
    );
}
