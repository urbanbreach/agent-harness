use super::*;

const LIFECYCLE_COPY_INSET_X: u16 = 2;

fn lifecycle_surface_copy_area(area: Rect) -> Rect {
    inset_rect(
        area,
        LIFECYCLE_COPY_INSET_X.min(area.width.saturating_sub(1) / 2),
        0,
    )
}

fn lifecycle_surface_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
) -> Block<'a> {
    ui_chrome::message_surface(theme, title, is_focused, theme.surface.panel_elevated)
}

fn render_lifecycle_copy_line(
    frame: &mut Frame,
    area: Rect,
    text: &str,
    style: Style,
    alignment: Alignment,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Paragraph::new(truncate_plain_text(text, usize::from(area.width)))
            .style(style)
            .alignment(alignment),
        area,
    );
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
    let surface = theme.surface.panel_elevated;
    let startup_card = app.startup_card_view_model();
    let block = lifecycle_surface_block(theme, "Home", list_focused);
    let content_area = lifecycle_surface_copy_area(block.inner(shell_area));

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
            Constraint::Min(0),
        ])
        .split(content_area);

    render_lifecycle_copy_line(
        frame,
        rows[0],
        theme.live_shell.startup.title,
        Style::default()
            .fg(if list_focused {
                theme.text.accent
            } else {
                theme.text.primary
            })
            .bg(surface)
            .add_modifier(Modifier::BOLD),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[1],
        &startup_card.metadata,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[2],
        theme.live_shell.startup.new_session_purpose,
        Style::default().fg(theme.text.primary).bg(surface),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[3],
        theme.live_shell.startup.secondary_hint,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Left,
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
    let surface = theme.surface.panel_elevated;
    let startup_card = app.startup_card_view_model();
    let block = lifecycle_surface_block(theme, "Session", false);
    let content_area = lifecycle_surface_copy_area(block.inner(shell_area));

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
            Constraint::Min(0),
        ])
        .split(content_area);

    render_lifecycle_copy_line(
        frame,
        rows[0],
        theme.live_shell.empty_state.title,
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[1],
        &startup_card.metadata,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[2],
        theme.live_shell.empty_state.value_prop,
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
        Alignment::Left,
    );
    render_lifecycle_copy_line(
        frame,
        rows[3],
        &help_row,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Left,
    );
}
