use super::*;

pub(super) fn render_terminal_panel(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = ui_chrome::divided_shell_surface(theme);
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    let entries = app.terminal_panel_entries();
    let title = terminal_panel_title(app, entries.len(), theme);
    let block = ui_chrome::panel_block(theme, title, app.focus == Focus::Terminal, surface);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let lines = if entries.is_empty() {
        terminal_panel_empty_lines(theme, surface)
    } else {
        terminal_panel_lines(&entries, theme, surface)
    };
    let lines = wrap_terminal_lines(lines, inner.width);
    let max_scroll = lines.len().saturating_sub(usize::from(inner.height));
    app.last_terminal_panel_max_scroll.set(max_scroll);
    let scroll_from_bottom = if app.terminal_panel_follow() {
        0
    } else {
        app.terminal_panel_scroll().min(max_scroll)
    };
    let top = max_scroll.saturating_sub(scroll_from_bottom);
    let text = Text::from(
        lines
            .into_iter()
            .skip(top)
            .take(usize::from(inner.height))
            .collect::<Vec<_>>(),
    );

    frame.render_widget(
        Paragraph::new(text)
            .style(ui_chrome::panel_style(surface, theme.text.primary))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn terminal_panel_title(app: &AppState, command_count: usize, theme: &Theme) -> Line<'static> {
    let meta_style = Style::default().fg(theme.text.secondary);
    let title_style = Style::default()
        .fg(theme.text.primary)
        .add_modifier(Modifier::BOLD);
    let count = match command_count {
        0 => "no commands".to_string(),
        1 => "1 command".to_string(),
        count => format!("{count} commands"),
    };
    let follow = if app.terminal_panel_follow() {
        "following"
    } else {
        "scrolled"
    };
    Line::from(vec![
        Span::styled("Terminal", title_style),
        Span::styled(format!("  {count} · {follow}"), meta_style),
    ])
}

fn terminal_panel_empty_lines(theme: &Theme, surface: Color) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "No shell commands have run in this session.",
            Style::default().fg(theme.text.secondary).bg(surface),
        )),
        Line::from(Span::styled(
            "When bash or shell commands execute, their command, status, stdout, stderr, exit code, and timing appear here.",
            Style::default().fg(theme.text.tertiary).bg(surface),
        )),
    ]
}

fn terminal_panel_lines(
    entries: &[crate::app::TerminalPanelEntry],
    theme: &Theme,
    surface: Color,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (idx, entry) in entries.iter().enumerate() {
        if idx > 0 {
            lines.push(Line::from(Span::styled("", Style::default().bg(surface))));
        }
        lines.push(terminal_command_line(entry, theme, surface));
        lines.push(terminal_meta_line(entry, theme, surface));
        push_output_lines(
            &mut lines,
            "stdout",
            entry.stdout.as_deref(),
            theme,
            surface,
            false,
        );
        push_output_lines(
            &mut lines,
            "stderr",
            entry.stderr.as_deref(),
            theme,
            surface,
            true,
        );
        if entry.truncated {
            let artifact = entry
                .output_artifact
                .as_deref()
                .map(|path| format!(" · full output {}", sanitize_terminal_text(path)))
                .unwrap_or_default();
            lines.push(Line::from(Span::styled(
                format!("… output truncated{artifact}"),
                Style::default().fg(theme.status.warning).bg(surface),
            )));
        }
    }
    lines
}

fn terminal_command_line(
    entry: &crate::app::TerminalPanelEntry,
    theme: &Theme,
    surface: Color,
) -> Line<'static> {
    Line::from(vec![
        Span::styled("$ ", Style::default().fg(theme.status.success).bg(surface)),
        Span::styled(
            sanitize_terminal_text(&entry.command),
            Style::default()
                .fg(theme.text.primary)
                .bg(surface)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn terminal_meta_line(
    entry: &crate::app::TerminalPanelEntry,
    theme: &Theme,
    surface: Color,
) -> Line<'static> {
    let status_color = match entry.status {
        crate::app::TerminalPanelStatus::Succeeded => theme.status.success,
        crate::app::TerminalPanelStatus::Failed => theme.status.error,
        crate::app::TerminalPanelStatus::Running => theme.status.info,
        crate::app::TerminalPanelStatus::PendingPermission
        | crate::app::TerminalPanelStatus::Queued => theme.status.warning,
    };
    let mut spans = vec![ui_chrome::status_badge(
        entry.status.label(),
        status_color,
        theme,
    )];
    let mut meta = Vec::new();
    if let Some(exit_code) = entry.exit_code {
        meta.push(format!("exit {exit_code}"));
    }
    if let Some(duration_ms) = entry.duration_ms {
        meta.push(format_duration_ms(duration_ms));
    }
    if let Some(cwd) = entry.cwd.as_deref() {
        meta.push(format!("cwd {}", sanitize_terminal_text(cwd)));
    }
    if !meta.is_empty() {
        spans.push(Span::styled(
            format!("  {}", meta.join(" · ")),
            Style::default().fg(theme.text.secondary).bg(surface),
        ));
    }
    Line::from(spans)
}

fn push_output_lines(
    lines: &mut Vec<Line<'static>>,
    label: &str,
    output: Option<&str>,
    theme: &Theme,
    surface: Color,
    error: bool,
) {
    let Some(output) = output else {
        return;
    };
    let color = if error {
        theme.status.error
    } else {
        theme.text.primary
    };
    for (idx, line) in output.lines().enumerate() {
        let prefix = if idx == 0 {
            format!("{label}> ")
        } else {
            "       ".to_string()
        };
        lines.push(Line::from(vec![
            Span::styled(prefix, Style::default().fg(theme.text.tertiary).bg(surface)),
            Span::styled(
                sanitize_terminal_text(line),
                Style::default().fg(color).bg(surface),
            ),
        ]));
    }
    if output.ends_with('\n') {
        lines.push(Line::from(Span::styled(
            "       ",
            Style::default().fg(theme.text.tertiary).bg(surface),
        )));
    }
}

fn wrap_terminal_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width).max(1);
    lines
        .into_iter()
        .flat_map(|line| wrap_terminal_line(line, width))
        .collect()
}

fn wrap_terminal_line(line: Line<'static>, width: usize) -> Vec<Line<'static>> {
    if line.spans.is_empty() || line.width() == 0 {
        return vec![line];
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0usize;

    for span in line.spans {
        let mut remaining = span.content.as_ref();
        while !remaining.is_empty() {
            if current_width >= width {
                rows.push(Line::from(std::mem::take(&mut current)));
                current_width = 0;
            }

            let available = width.saturating_sub(current_width).max(1);
            let mut chunk = take_width_prefix(remaining, available);
            if chunk.is_empty() {
                chunk = remaining
                    .char_indices()
                    .nth(1)
                    .map(|(index, _)| &remaining[..index])
                    .unwrap_or(remaining);
            }

            current_width = current_width.saturating_add(display_width(chunk));
            current.push(Span::styled(chunk.to_string(), span.style));
            remaining = &remaining[chunk.len()..];
        }
    }

    if !current.is_empty() {
        rows.push(Line::from(current));
    }
    rows
}

fn sanitize_terminal_text(text: &str) -> String {
    crate::text::replace_control_chars_except_tabs(text)
}

fn format_duration_ms(duration_ms: u64) -> String {
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else {
        format!("{:.1} s", duration_ms as f64 / 1_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_wrap_counts_display_rows_before_scroll() {
        let style = Style::default();
        let rows = wrap_terminal_lines(
            vec![Line::from(vec![
                Span::styled("stdout> ".to_string(), style),
                Span::styled("abcdefghij".to_string(), style),
            ])],
            10,
        );

        let rendered = rows
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(rendered, vec!["stdout> ab", "cdefghij"]);
    }

    #[test]
    fn terminal_truncation_artifact_paths_are_sanitized() {
        assert_eq!(
            sanitize_terminal_text("artifacts/\u{1b}]52;c;secret\u{7}.txt"),
            "artifacts/ ]52;c;secret .txt"
        );
    }
}
