use super::*;

pub(super) fn render_transcript_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if !app.replay_mode {
        let inner_area = inset_rect(area, theme.live_shell.rhythm.transcript_gutter_x, 0);

        if app.startup_shell_visible() {
            render_startup_lifecycle_surface(frame, app, inner_area, theme);
            return;
        }

        if app.post_run_handoff_visible() {
            render_post_run_handoff_surface(frame, app, inner_area, theme);
            return;
        }

        if live_empty_state_visible(app) {
            render_live_empty_state(frame, app, inner_area, theme);
            return;
        }

        let lines = build_transcript_lines(app, theme);
        let transcript_scroll = transcript_scroll_offset(app, &lines, inner_area);
        let content = Text::from(lines);

        frame.render_widget(
            Paragraph::new(content)
                .style(panel_style(theme.surface.shell, theme.text.primary))
                .scroll((transcript_scroll, 0))
                .wrap(Wrap { trim: false }),
            inner_area,
        );
        return;
    }

    let is_focused = transcript_surface_focused(app);

    let title = if app.replay_mode {
        format!(
            "Transcript{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    } else {
        format!(
            "Conversation{}{}",
            if is_focused { " (focused)" } else { "" },
            if app.follow_mode { " (following)" } else { "" }
        )
    };

    let surface = theme.surface.panel;
    let block = panel_block(theme, title, is_focused, surface);

    let inner_area = inset_rect(
        block.inner(area),
        theme.live_shell.rhythm.transcript_gutter_x,
        theme.live_shell.rhythm.transcript_gutter_y,
    );

    frame.render_widget(block, area);

    if live_empty_state_visible(app) {
        render_live_empty_state(frame, app, inner_area, theme);
        return;
    }

    let lines = build_transcript_lines(app, theme);
    let transcript_scroll = transcript_scroll_offset(app, &lines, inner_area);
    let content = Text::from(lines);

    frame.render_widget(
        Paragraph::new(content)
            .style(panel_style(surface, theme.text.primary))
            .scroll((transcript_scroll, 0))
            .wrap(Wrap { trim: false }),
        inner_area,
    );
}

pub(crate) fn build_transcript_lines(app: &AppState, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let redesign_active = app.transcript_first_shell_redesign_active();

    for (index, activity) in app.activities.iter().enumerate() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        append_activity_lines(
            &mut lines,
            activity,
            index == app.selected_activity_index,
            theme,
        );
    }

    for (_permission_id, summary) in app.transcript_pending_permissions() {
        if !lines.is_empty() {
            lines.push(Line::from(""));
        }
        append_pending_permission_lines(&mut lines, &summary, theme, redesign_active);
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Waiting for first turn…",
            Style::default().fg(theme.text.secondary),
        )));
    }

    lines
}

fn append_activity_lines(
    lines: &mut Vec<Line<'static>>,
    activity: &ActivityEntry,
    is_selected: bool,
    theme: &Theme,
) {
    let header_style = transcript_label_style(theme, is_selected);
    let meta_style = muted_meta_style(theme);
    let body_prefix = format!("  {} ", theme.live_shell.transcript_glyphs.card_mid);

    if let Some(user_msg) = &activity.user_message {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} ", theme.live_shell.transcript_glyphs.user_marker),
                Style::default().fg(theme.text.accent),
            ),
            Span::styled("user", header_style),
            Span::styled(
                format!(" · {}", request_id_label(&activity.request_id)),
                meta_style,
            ),
        ]));
        append_prefixed_text_block(
            lines,
            &user_msg.text,
            &body_prefix,
            transcript_prefix_style(theme),
            Style::default().fg(theme.text.primary),
        );
    }

    let (assistant_icon, assistant_color, assistant_status) = match activity.status {
        ActivityStatus::Streaming => (
            theme.live_shell.glyphs.streaming,
            theme.status.info,
            "streaming…",
        ),
        ActivityStatus::Done => (theme.live_shell.glyphs.done, theme.status.success, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };
    let mut assistant_meta = Vec::new();
    if !activity.provider_id.is_empty() || !activity.model_id.is_empty() {
        assistant_meta.push(format!(
            "{}/{}",
            if activity.provider_id.is_empty() {
                "-"
            } else {
                activity.provider_id.as_str()
            },
            if activity.model_id.is_empty() {
                "-"
            } else {
                activity.model_id.as_str()
            }
        ));
    }
    let mut assistant_line = vec![
        Span::styled(
            format!("{} ", assistant_icon),
            Style::default().fg(assistant_color),
        ),
        status_badge(assistant_status, assistant_color, theme),
        Span::raw(" "),
        Span::styled("assistant", header_style),
    ];
    if !assistant_meta.is_empty() {
        assistant_line.push(Span::styled(
            format!(" · {}", assistant_meta.join(" · ")),
            meta_style,
        ));
    }
    if is_selected {
        assistant_line.push(Span::styled(
            " · current",
            Style::default().fg(theme.text.accent),
        ));
    }
    lines.push(Line::from(assistant_line));

    if !activity.transcript_text.is_empty() {
        append_prefixed_text_block(
            lines,
            &activity.transcript_text,
            &body_prefix,
            transcript_prefix_style(theme),
            Style::default().fg(theme.text.primary),
        );
    } else if activity.status == ActivityStatus::Streaming {
        append_prefixed_text_block(
            lines,
            "Waiting for response…",
            &body_prefix,
            transcript_prefix_style(theme),
            Style::default().fg(theme.text.secondary),
        );
    }

    if let Some(error) = &activity.error_message {
        lines.push(Line::from(vec![
            Span::styled("  ↳ ", Style::default().fg(theme.status.error)),
            Span::styled(error.clone(), Style::default().fg(theme.status.error)),
        ]));
    }

    for tool_call in &activity.tool_calls {
        append_tool_call_lines(lines, tool_call, theme);
    }
}

fn append_tool_call_lines(
    lines: &mut Vec<Line<'static>>,
    tool_call: &crate::app::ToolCallEntry,
    theme: &Theme,
) {
    let glyphs = &theme.live_shell.transcript_glyphs;
    let (status_icon, status_color, _status_label, _) = tool_status_tokens(tool_call.status, theme);

    lines.push(Line::from(vec![
        Span::styled(
            format!("  {} ", glyphs.card_top),
            transcript_prefix_style(theme),
        ),
        Span::styled("tool ", muted_meta_style(theme)),
        Span::styled(
            tool_call.tool_id.clone(),
            Style::default()
                .fg(theme.text.primary)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        tool_status_badge(tool_call.status, theme),
    ]));

    if let Some(state_summary) = tool_state_summary(tool_call) {
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            "state",
            state_summary,
            tool_detail_label_style("state", theme, tool_call.status),
            subdued_payload_style(theme),
            theme,
        );
    }

    if let Some(args_summary) = compact_inline_payload(&tool_call.args_summary, 96) {
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            "args",
            &args_summary,
            tool_detail_label_style("args", theme, tool_call.status),
            subdued_payload_style(theme),
            theme,
        );
    }

    if let Some(output) = tool_call
        .truncated_output
        .as_deref()
        .and_then(|output| compact_inline_payload(output, 96))
    {
        let label = if tool_call.status == ToolCallDisplayStatus::Failed {
            "error"
        } else {
            "result"
        };
        let output_style = if tool_call.status == ToolCallDisplayStatus::Failed {
            Style::default().fg(theme.status.error)
        } else {
            subdued_payload_style(theme)
        };
        append_tool_card_row(
            lines,
            &format!("  {} ", glyphs.card_mid),
            label,
            &output,
            tool_detail_label_style(label, theme, tool_call.status),
            output_style,
            theme,
        );
    }

    let footer = tool_footer_summary(tool_call);
    let status_line = vec![
        Span::styled("  └ ".to_string(), transcript_prefix_style(theme)),
        Span::styled(
            format!("{} ", status_icon),
            Style::default().fg(status_color),
        ),
        Span::styled(footer, Style::default().fg(theme.text.tertiary)),
    ];
    lines.push(Line::from(status_line));
}

fn append_pending_permission_lines(
    lines: &mut Vec<Line<'static>>,
    summary: &str,
    theme: &Theme,
    redesign_active: bool,
) {
    lines.push(Line::from(vec![
        Span::styled(
            format!("{} ", theme.live_shell.glyphs.pending_permission),
            Style::default().fg(theme.status.warning),
        ),
        status_badge("requested", theme.status.warning, theme),
        Span::raw(" "),
        Span::styled(
            "permission",
            Style::default()
                .fg(theme.status.warning)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if redesign_active {
        append_prefixed_text_block(
            lines,
            summary,
            &format!("  {} ", theme.live_shell.transcript_glyphs.card_mid),
            transcript_prefix_style(theme),
            Style::default().fg(theme.text.primary),
        );
    } else {
        append_text_block(lines, summary, theme.text.primary, "  ");
    }
}

pub(super) fn append_text_block<'a>(
    lines: &mut Vec<Line<'a>>,
    text: &str,
    color: ratatui::style::Color,
    prefix: &str,
) {
    for line in text.lines() {
        let body = if line.is_empty() {
            prefix.to_string()
        } else {
            format!("{prefix}{line}")
        };
        lines.push(Line::from(Span::styled(body, Style::default().fg(color))));
    }

    if text.is_empty() {
        lines.push(Line::from(Span::styled(
            prefix.to_string(),
            Style::default().fg(color),
        )));
    }
}

pub fn hovered_wheel_target(
    app: &AppState,
    area: Rect,
    column: u16,
    row: u16,
) -> Option<WheelTarget> {
    let hit_areas = FrameLayoutPlan::for_app(app, area).wheel_hit_areas;
    if hit_areas
        .inspector
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return Some(WheelTarget::Inspector);
    }
    if hit_areas
        .overlay
        .is_some_and(|area| rect_contains(area, column, row))
    {
        return None;
    }
    hit_areas
        .transcript
        .filter(|area| rect_contains(*area, column, row))
        .map(|_| WheelTarget::Transcript)
}

fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

fn transcript_scroll_offset(app: &AppState, lines: &[Line<'static>], inner_area: Rect) -> u16 {
    let viewport_height = usize::from(inner_area.height);
    let viewport_width = usize::from(inner_area.width.max(1));
    if viewport_height == 0 {
        return 0;
    }

    let total_rows = transcript_visual_rows(lines, viewport_width);
    let max_scroll = total_rows.saturating_sub(viewport_height);
    if max_scroll == 0 {
        return 0;
    }

    if app.follow_mode {
        return u16::try_from(max_scroll).unwrap_or(u16::MAX);
    }

    let scroll_back = usize::from(app.transcript_scroll);
    let scroll = max_scroll.saturating_sub(scroll_back);
    u16::try_from(scroll).unwrap_or(u16::MAX)
}

fn transcript_visual_rows(lines: &[Line<'static>], viewport_width: usize) -> usize {
    lines
        .iter()
        .map(|line| {
            let width = line.width();
            if width == 0 {
                1
            } else {
                width.div_ceil(viewport_width)
            }
        })
        .sum()
}

fn transcript_surface_focused(app: &AppState) -> bool {
    !app.replay_mode
        && app.active_tab == Tab::Run
        && app.focus == Focus::Details
        && !app.details_drawer_open()
}

fn append_tool_card_row(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    label: &str,
    value: &str,
    label_style: Style,
    value_style: Style,
    theme: &Theme,
) {
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(format!("{label:<6}"), label_style));
    if !value.is_empty() {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(value.to_string(), value_style));
    }
    lines.push(Line::from(spans));
}

fn append_prefixed_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    prefix: &str,
    prefix_style: Style,
    body_style: Style,
) {
    for line in text.split('\n') {
        let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
        if !line.is_empty() {
            spans.push(Span::styled(line.to_string(), body_style));
        }
        lines.push(Line::from(spans));
    }

    if text.is_empty() {
        lines.push(Line::from(Span::styled(prefix.to_string(), prefix_style)));
    }
}
