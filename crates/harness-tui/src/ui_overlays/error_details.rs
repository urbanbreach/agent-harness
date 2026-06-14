use super::*;

pub(super) fn render_error_details_overlay(
    frame: &mut Frame,
    app: &AppState,
    theme: &Theme,
    root: Rect,
) {
    let Some(details) = app.last_error_details_view_model() else {
        return;
    };
    render_overlay_dim_backdrop(frame, root);
    let width = root.width.saturating_sub(12).min(84);
    let height = root.height.saturating_sub(6).min(14);
    if width < 40 || height < 8 {
        return;
    }
    let area = centered_rect(root, width, height);
    let surface = theme.surface.panel_elevated;
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.status.error))
            .style(Style::default().bg(surface)),
        area,
    );
    let content = inset_rect(area, 2, 1);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Error details",
                Style::default()
                    .fg(theme.text.primary)
                    .bg(surface)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                "  esc",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
        ]),
        Line::default(),
        Line::from(vec![
            Span::styled(
                "Category: ",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
            Span::styled(
                details.category,
                Style::default().fg(theme.status.error).bg(surface),
            ),
        ]),
    ];
    if let Some(request_id) = details.request_id.as_deref() {
        lines.push(Line::from(vec![
            Span::styled(
                "Request:  ",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
            Span::styled(
                request_id.to_string(),
                Style::default().fg(theme.text.primary).bg(surface),
            ),
        ]));
    }
    lines.extend([
        Line::from(vec![
            Span::styled(
                "Message:  ",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
            Span::styled(
                details.message,
                Style::default().fg(theme.text.primary).bg(surface),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Recovery: ",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
            Span::styled(
                details.recovery_hint,
                Style::default().fg(theme.text.primary).bg(surface),
            ),
        ]),
        Line::default(),
    ]);
    if details.replay_mode {
        lines.push(Line::from(Span::styled(
            "Replay read-only · inspect and resume from a live session.",
            Style::default().fg(theme.text.secondary).bg(surface),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.text.primary).bg(surface)),
            Span::styled(
                " Resubmit last prompt through the normal composer path",
                Style::default().fg(theme.text.secondary).bg(surface),
            ),
        ]));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: true }),
        content,
    );
}

fn centered_rect(root: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        root.x + root.width.saturating_sub(width) / 2,
        root.y + root.height.saturating_sub(height) / 2,
        width,
        height,
    )
}
