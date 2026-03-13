use super::*;

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

pub(super) fn render_startup_lifecycle_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let shell_area = crate::layout::startup_shell_area(area, theme);
    let list_focused = app.focus == Focus::List;
    let surface = theme.surface.shell;
    let content_area = inset_rect(shell_area, 0, 1);

    frame.render_widget(Block::default().style(Style::default().bg(surface)), shell_area);

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
            Constraint::Min(0),
        ])
        .split(content_area);

    let startup_card = app.startup_card_view_model();

    frame.render_widget(
        Paragraph::new(theme.live_shell.startup.title)
            .style(
                Style::default()
                    .fg(if list_focused {
                        theme.text.accent
                    } else {
                        theme.text.primary
                    })
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            )
            .alignment(Alignment::Center),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(startup_card.metadata)
            .style(Style::default().fg(theme.text.tertiary).bg(surface))
            .alignment(Alignment::Center),
        rows[1],
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
