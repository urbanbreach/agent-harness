use super::*;

const LIFECYCLE_COPY_INSET_X: u16 = 3;
const STARTUP_LOGO_LINES: [(&str, &str); 3] = [
    ("╻ ╻  ┏━┓  ┏━┓  ┏┓╻", "┏━╸  ┏━┓  ┏━┓"),
    ("┣━┫  ┣━┫  ┣┳┛  ┃┗┫", "┣╸   ┗━┓  ┗━┓"),
    ("╹ ╹  ╹ ╹  ╹┗╸  ╹ ╹", "┗━╸  ┗━┛  ┗━┛"),
];

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

fn startup_logo_lines(content_area: Rect, theme: &Theme) -> Text<'static> {
    if content_area.width < 40 {
        return Text::from(vec![Line::from(Span::styled(
            theme.live_shell.startup.title,
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ))]);
    }

    Text::from(
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
            .collect::<Vec<_>>(),
    )
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
        && app.transcript_pending_permissions().is_empty()
        && app.prompt_buffer.is_empty()
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

pub(crate) fn render_startup_lifecycle_flow(
    frame: &mut Frame,
    app: &AppState,
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
    let runtime_summary = app.runtime_context_primary_summary();
    let runtime_detail = startup_runtime_detail(app);
    let summary_visible = content_area.height >= logo_height.saturating_add(1);
    let detail_visible =
        runtime_detail.is_some() && content_area.height >= logo_height.saturating_add(2);
    let purpose_visible = content_area.height
        >= logo_height
            .saturating_add(u16::from(summary_visible))
            .saturating_add(u16::from(detail_visible))
            .saturating_add(1);
    let content_height = logo_height
        .saturating_add(u16::from(summary_visible))
        .saturating_add(u16::from(detail_visible))
        .saturating_add(u16::from(purpose_visible));
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
            Constraint::Length(u16::from(summary_visible)),
            Constraint::Length(u16::from(detail_visible)),
            Constraint::Length(u16::from(purpose_visible)),
            Constraint::Min(0),
        ])
        .split(content_area);

    frame.render_widget(
        Paragraph::new(startup_logo_lines(content_area, theme))
            .style(Style::default().bg(surface))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        rows[1],
    );

    if summary_visible && rows[2].height > 0 {
        render_lifecycle_copy_line(
            frame,
            rows[2],
            &runtime_summary,
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
            Alignment::Center,
        );
    }

    if detail_visible && rows[3].height > 0 {
        if let Some(detail) = runtime_detail.as_deref() {
            render_lifecycle_copy_line(
                frame,
                rows[3],
                detail,
                Style::default().fg(theme.text.secondary).bg(surface),
                Alignment::Center,
            );
        }
    }

    if purpose_visible && rows[4].height > 0 {
        render_lifecycle_copy_line(
            frame,
            rows[4],
            theme.live_shell.startup.new_session_purpose,
            Style::default().fg(theme.text.secondary).bg(surface),
            Alignment::Center,
        );
    }
}

fn startup_runtime_detail(app: &AppState) -> Option<String> {
    let mut segments = Vec::new();

    if let Some(provider) = app
        .runtime_context_provider_display()
        .as_deref()
        .map(str::trim)
        .filter(|provider| !provider.is_empty())
    {
        segments.push(format!("Provider {provider}"));
    }

    if let Some(mode) = app
        .launch_mode_label()
        .map(str::trim)
        .filter(|mode| !mode.is_empty())
    {
        segments.push(mode.to_string());
    }

    (!segments.is_empty()).then(|| segments.join(" · "))
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

    let newline = match app
        .keymap
        .get_binding_strs(Action::InsertNewline)
        .as_slice()
    {
        [] => "-".to_string(),
        [binding] => binding.clone(),
        [first, second, ..] => format!("{first}/{second}"),
    };
    let help_row = [
        app.keymap.get_binding_label(Action::SubmitPrompt, "send"),
        format!("{newline} newline"),
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
