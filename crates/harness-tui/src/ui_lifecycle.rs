use super::*;

const LIFECYCLE_COPY_INSET_X: u16 = 3;
const STARTUP_LOGO_LINES: [(&str, &str); 3] = [
    ("╻ ╻  ┏━┓  ┏━┓  ┏┓╻", "┏━╸  ┏━┓  ┏━┓"),
    ("┣━┫  ┣━┫  ┣┳┛  ┃┗┫", "┣╸   ┗━┓  ┗━┓"),
    ("╹ ╹  ╹ ╹  ╹┗╸  ╹ ╹", "┗━╸  ┗━┛  ┗━┛"),
];

#[derive(Debug, Clone)]
pub(super) struct LifecycleSelectionSurface {
    pub viewport: Rect,
    pub text_rows: Vec<LifecycleSelectableText>,
}

#[derive(Debug, Clone)]
pub(super) struct LifecycleSelectableText {
    pub row: usize,
    pub max_height: u16,
    pub line: Line<'static>,
    pub alignment: Alignment,
}

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

fn startup_logo_lines(content_area: Rect, theme: &Theme) -> Vec<Line<'static>> {
    if content_area.width < 40 {
        return vec![Line::from(Span::styled(
            theme.live_shell.startup.title,
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ))];
    }

    STARTUP_LOGO_LINES
        .iter()
        .map(|(left, right)| {
            Line::from(vec![
                Span::styled(*left, Style::default().fg(theme.text.secondary)),
                Span::raw("  "),
                Span::styled(
                    *right,
                    Style::default()
                        .fg(theme.text.primary)
                        .add_modifier(Modifier::BOLD),
                ),
            ])
        })
        .collect::<Vec<_>>()
}

fn startup_logo_height(content_area: Rect) -> u16 {
    if content_area.width < 40 {
        1
    } else {
        STARTUP_LOGO_LINES.len() as u16
    }
}

fn empty_state_examples_text(theme: &Theme) -> String {
    theme
        .live_shell
        .empty_state
        .example_prompts
        .iter()
        .map(|prompt| format!("“{}”", prompt.prompt))
        .collect::<Vec<_>>()
        .join(" · ")
}

pub(super) fn live_empty_state_visible(app: &AppState) -> bool {
    !app.replay_mode
        && !app.startup_shell_visible()
        && app.activities.is_empty()
        && app.active_permission_view().is_none()
        && app.transcript_pending_permissions().is_empty()
        && app.composer.prompt_buffer.is_empty()
}

pub(super) fn startup_shell_visible(app: &AppState) -> bool {
    app.startup_shell_visible()
}

pub(crate) fn render_startup_lifecycle_surface(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    let shell_area = crate::layout::startup_shell_area(area, theme);
    render_startup_lifecycle_flow(frame, app, shell_area, theme);
}

pub(super) fn startup_lifecycle_selection_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    let shell_area = crate::layout::startup_shell_area(area, theme);
    startup_lifecycle_flow_selection_surface(app, shell_area, theme)
}

pub(crate) fn render_startup_lifecycle_flow(
    frame: &mut Frame,
    _app: &AppState,
    area: Rect,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let shell_area = area;
    let surface = theme.surface.shell;
    let content_area = lifecycle_surface_copy_area(shell_area);

    let logo_height = startup_logo_height(content_area);
    let content_height = logo_height;
    let top_gap = content_area.height.saturating_sub(content_height) / 2;

    frame.render_widget(
        Block::default().style(Style::default().bg(surface)),
        shell_area,
    );

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_gap),
            Constraint::Length(logo_height),
            Constraint::Min(0),
        ])
        .split(content_area);

    let logo_lines = startup_logo_lines(content_area, theme);

    frame.render_widget(
        Paragraph::new(Text::from(logo_lines))
            .style(Style::default().bg(surface))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        rows[1],
    );
}

fn startup_lifecycle_flow_selection_surface(
    _app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let content_area = lifecycle_surface_copy_area(area);
    if content_area.width == 0 || content_area.height == 0 {
        return None;
    }

    let logo_height = startup_logo_height(content_area);
    let content_height = logo_height;
    let top_gap = content_area.height.saturating_sub(content_height) / 2;
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_gap),
            Constraint::Length(logo_height),
            Constraint::Min(0),
        ])
        .split(content_area);

    let mut text_rows = Vec::new();
    for (idx, line) in startup_logo_lines(content_area, theme)
        .into_iter()
        .enumerate()
    {
        text_rows.push(LifecycleSelectableText {
            row: usize::from(rows[1].y.saturating_sub(content_area.y)).saturating_add(idx),
            max_height: 1,
            line,
            alignment: Alignment::Center,
        });
    }

    Some(LifecycleSelectionSurface {
        viewport: content_area,
        text_rows,
    })
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

    let help_row = format!(
        "{}  {}",
        app.current_context_window_tokens()
            .map(|_| "0 (0%)")
            .unwrap_or("0"),
        app.keymap.get_binding_label(Action::Palette, "commands")
    );

    let shell_area = live_empty_state_area(area, theme);
    let surface = theme.surface.panel_elevated;
    let startup_card = app.startup_card_view_model();
    let block = lifecycle_surface_block(theme, "Session", false);
    let content_area = lifecycle_surface_copy_area(block.inner(shell_area));
    let example_prompts = empty_state_examples_text(theme);
    let examples_height = u16::from(content_area.width < 76).saturating_add(1);

    frame.render_widget(block, shell_area);

    if content_area.width == 0 || content_area.height == 0 {
        return;
    }

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(examples_height),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(content_area);

    render_lifecycle_copy_line(
        frame,
        rows[1],
        theme.live_shell.empty_state.title,
        Style::default()
            .fg(theme.text.accent)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
        Alignment::Center,
    );
    render_lifecycle_copy_line(
        frame,
        rows[2],
        &startup_card.metadata,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Center,
    );
    render_lifecycle_copy_line(
        frame,
        rows[3],
        theme.live_shell.empty_state.value_prop,
        Style::default()
            .fg(theme.text.primary)
            .bg(surface)
            .add_modifier(Modifier::BOLD),
        Alignment::Center,
    );
    frame.render_widget(
        Paragraph::new(example_prompts)
            .style(Style::default().fg(theme.text.primary).bg(surface))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        rows[4],
    );
    render_lifecycle_copy_line(
        frame,
        rows[5],
        &help_row,
        Style::default().fg(theme.text.secondary).bg(surface),
        Alignment::Center,
    );
}

pub(super) fn live_empty_state_selection_surface(
    app: &AppState,
    area: Rect,
    theme: &Theme,
) -> Option<LifecycleSelectionSurface> {
    if area.width == 0 || area.height == 0 {
        return None;
    }

    let help_row = format!(
        "{}  {}",
        app.current_context_window_tokens()
            .map(|_| "0 (0%)")
            .unwrap_or("0"),
        app.keymap.get_binding_label(Action::Palette, "commands")
    );

    let shell_area = live_empty_state_area(area, theme);
    let block = lifecycle_surface_block(theme, "Session", false);
    let content_area = lifecycle_surface_copy_area(block.inner(shell_area));
    if content_area.width == 0 || content_area.height == 0 {
        return None;
    }

    let startup_card = app.startup_card_view_model();
    let example_prompts = empty_state_examples_text(theme);
    let examples_height = u16::from(content_area.width < 76).saturating_add(1);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(examples_height),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(content_area);

    let mut text_rows = Vec::new();
    for (row_area, max_height, text) in [
        (
            rows[1],
            rows[1].height,
            theme.live_shell.empty_state.title.to_string(),
        ),
        (rows[2], rows[2].height, startup_card.metadata),
        (
            rows[3],
            rows[3].height,
            theme.live_shell.empty_state.value_prop.to_string(),
        ),
        (rows[5], rows[5].height, help_row),
    ] {
        if max_height == 0 {
            continue;
        }
        text_rows.push(LifecycleSelectableText {
            row: usize::from(row_area.y.saturating_sub(content_area.y)),
            max_height,
            line: Line::from(Span::raw(truncate_plain_text(
                &text,
                usize::from(row_area.width),
            ))),
            alignment: Alignment::Center,
        });
    }

    if rows[4].height > 0 {
        text_rows.push(LifecycleSelectableText {
            row: usize::from(rows[4].y.saturating_sub(content_area.y)),
            max_height: rows[4].height,
            line: Line::from(Span::raw(example_prompts)),
            alignment: Alignment::Center,
        });
    }

    Some(LifecycleSelectionSurface {
        viewport: content_area,
        text_rows,
    })
}
