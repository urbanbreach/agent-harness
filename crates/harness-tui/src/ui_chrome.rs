use super::*;

pub(super) fn render_header(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    text_area: Rect,
    theme: &Theme,
) {
    if startup_shell_visible(app)
        || live_empty_state_visible(app)
        || app.continued_post_run_handoff_active()
    {
        return;
    }

    let run_id = app.run_id().unwrap_or("unknown");

    let header_text = if app.replay_mode {
        let session_path = app
            .session_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "unknown".to_string());
        format!(
            "Replay · {run_id} · {session_path} · {} ev",
            app.events.len()
        )
    } else {
        let metadata = format!(
            "run {run_id} · {}/{} · {}",
            app.active_profile(),
            app.active_provider(),
            app.current_model_label()
        );
        app.launch_mode_label()
            .map(|label| format!("{label} · {metadata}"))
            .unwrap_or(metadata)
    };

    if app.replay_mode {
        let style = Style::default()
            .fg(theme.text.secondary)
            .bg(theme.surface.shell);
        frame.render_widget(Block::default().style(style), area);
        frame.render_widget(Paragraph::new(header_text).style(style), text_area);
    } else {
        frame.render_widget(
            Paragraph::new(header_text).style(Style::default().fg(theme.text.tertiary)),
            text_area,
        );
    }
}

pub(super) fn render_tabs(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let titles: Vec<Line> = app
        .surface_registry()
        .iter()
        .enumerate()
        .map(|(i, surface)| {
            let style = if i == replay_tab_index(app.active_tab) {
                Style::default()
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text.secondary)
            };
            Line::from(Span::styled(surface.label, style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(panel_block(theme, "Tabs", false, theme.surface.panel))
        .select(replay_tab_index(app.active_tab))
        .style(panel_style(theme.surface.panel, theme.text.tertiary))
        .highlight_style(panel_style(theme.surface.panel, theme.text.accent));

    frame.render_widget(tabs, area);
}

fn replay_tab_index(active_tab: Tab) -> usize {
    match active_tab {
        Tab::Run | Tab::Details => 0,
        Tab::Events => 1,
        Tab::Diff => 2,
        Tab::Help => 3,
    }
}

pub(super) fn render_footer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    text_area: Rect,
    theme: &Theme,
) {
    let separator = " ".repeat(theme.live_shell.rhythm.status_separator as usize);
    let footer_hints = app.footer_hints_view_model();
    let hint_text = footer_hints
        .hints
        .iter()
        .map(|hint| app.keymap.get_binding_label(hint.action, hint.label))
        .collect::<Vec<_>>()
        .join(&separator);

    if !app.transcript_first_shell_redesign_active()
        || area.width < primary_shell_context_width(theme)
    {
        let style = Style::default().fg(theme.text.tertiary);
        let hint_text = footer_hints.prefix.map_or(hint_text.clone(), |prefix| {
            format!("{prefix}{separator}{hint_text}")
        });

        if app.replay_mode {
            let replay_style = style.bg(theme.surface.shell);
            frame.render_widget(Block::default().style(replay_style), area);
            frame.render_widget(Paragraph::new(hint_text).style(replay_style), text_area);
        } else {
            frame.render_widget(Paragraph::new(hint_text).style(style), text_area);
        }
        return;
    }

    let base_style = Style::default()
        .fg(theme.text.tertiary)
        .bg(theme.surface.shell);
    let gap = " ".repeat(theme.live_shell.rhythm.footer_prefix_gap as usize);
    let (context_label, context_color) = footer_context(app, theme);
    let context_badge = status_badge(context_label, context_color, theme);
    let continued_badge = footer_hints
        .prefix
        .map(|prefix| status_badge(prefix, theme.border.strong, theme));

    let mut spans = Vec::new();
    let mut leading = Vec::new();
    if let Some(badge) = continued_badge.clone() {
        leading.push(badge);
        leading.push(Span::styled(gap.clone(), base_style));
    }
    leading.push(context_badge.clone());

    let hint_gap = if hint_text.is_empty() {
        None
    } else {
        Some(Span::styled(gap.clone(), base_style))
    };
    let hint_span = if hint_text.is_empty() {
        None
    } else {
        Some(Span::styled(hint_text, base_style))
    };

    let full_width = status_strip_width(&leading)
        + hint_gap
            .as_ref()
            .map_or(0, |gap| gap.content.chars().count())
        + hint_span
            .as_ref()
            .map_or(0, |hint| hint.content.chars().count());
    let context_width = context_badge.content.chars().count();
    let hint_width = hint_gap
        .as_ref()
        .map_or(0, |gap| gap.content.chars().count())
        + hint_span
            .as_ref()
            .map_or(0, |hint| hint.content.chars().count());

    if full_width <= usize::from(area.width) {
        spans.extend(leading);
    } else if context_width + hint_width <= usize::from(area.width) {
        spans.push(context_badge);
    }

    if let Some(gap) = hint_gap {
        if !spans.is_empty() {
            spans.push(gap);
        }
    }
    if let Some(hint_span) = hint_span {
        spans.push(hint_span);
    }

    frame.render_widget(Block::default().style(base_style), area);
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(base_style),
        text_area,
    );
}

pub(super) fn render_status_strip(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    let state = app.runtime_state();
    let base_style = Style::default()
        .fg(theme.text.secondary)
        .bg(theme.surface.shell);
    let redesign_active = app.transcript_first_shell_redesign_active();
    let mut spans: Vec<Span<'static>> = vec![
        status_badge(
            state.kind.label(),
            runtime_state_color(state.kind, theme),
            theme,
        ),
        Span::styled("  ", base_style),
    ];

    if redesign_active && area.width >= primary_shell_context_width(theme) {
        let (context_label, context_color) = status_context(app, theme, state.kind);
        spans.push(status_badge(context_label, context_color, theme));
        spans.push(Span::styled("  ", base_style));
    }

    spans.push(Span::styled(state.summary, base_style));

    if !app.replay_mode {
        if redesign_active {
            append_orchestration_status(&mut spans, app, area.width, base_style, theme);
        } else {
            append_orchestration_status_legacy(&mut spans, app, area.width, base_style, theme);
        }
    }

    if let Some((tool_summary, tool_color)) = tool_status_summary(app) {
        let separator = "  ·  ";
        let available = usize::from(area.width)
            .saturating_sub(status_strip_width(&spans))
            .saturating_sub(separator.chars().count());
        if available > 10 {
            spans.push(Span::styled(separator, base_style));
            spans.push(Span::styled(
                truncate_plain_text(&tool_summary, available),
                Style::default()
                    .fg(tool_color)
                    .bg(theme.surface.shell)
                    .add_modifier(Modifier::BOLD),
            ));
        }
    }

    let status_line = Line::from(spans);

    frame.render_widget(Paragraph::new(status_line).style(base_style), area);
}

fn append_orchestration_status(
    spans: &mut Vec<Span<'static>>,
    app: &AppState,
    width: u16,
    base_style: Style,
    theme: &Theme,
) {
    let summary = app.orchestration_summary();
    let latest_warning = app.orchestration_latest_warning();
    let summary_segment = format!(
        "  ·  orch {}a {}q {}r {}s",
        summary.active_agents, summary.queued, summary.running, summary.stale
    );

    if !append_status_segment_if_fits(spans, width, summary_segment, base_style) {
        return;
    }

    let Some(latest_warning) = latest_warning else {
        return;
    };

    if width < primary_shell_context_width(theme) {
        return;
    }

    let available = usize::from(width).saturating_sub(status_strip_width(spans));
    let warning_segment = format!(" · warn {latest_warning}");
    if warning_segment.chars().count() > available {
        return;
    }

    let warning_style = Style::default()
        .fg(theme.status.warning)
        .bg(theme.surface.shell)
        .add_modifier(Modifier::BOLD);
    spans.push(Span::styled(warning_segment, warning_style));
}

fn append_orchestration_status_legacy(
    spans: &mut Vec<Span<'static>>,
    app: &AppState,
    width: u16,
    base_style: Style,
    theme: &Theme,
) {
    let summary = app.orchestration_summary();
    let latest_warning = app.orchestration_latest_warning();
    let count_segments = [
        format!("  ·  agents {}", summary.active_agents),
        format!(" · queued {}", summary.queued),
        format!(" · running {}", summary.running),
        format!(" · stale {}", summary.stale),
    ];

    let mut appended_all_counts = true;
    for segment in count_segments {
        if !append_status_segment_if_fits(spans, width, segment, base_style) {
            appended_all_counts = false;
            break;
        }
    }

    if !appended_all_counts {
        return;
    }

    let Some(latest_warning) = latest_warning else {
        return;
    };

    let available = usize::from(width).saturating_sub(status_strip_width(spans));
    let warning_prefix_width = " · warn ".chars().count();
    if available <= warning_prefix_width {
        return;
    }

    let warning_style = Style::default()
        .fg(theme.status.warning)
        .bg(theme.surface.shell)
        .add_modifier(Modifier::BOLD);
    let warning_segment = truncate_plain_text(&format!(" · warn {latest_warning}"), available);
    spans.push(Span::styled(warning_segment, warning_style));
}

fn append_status_segment_if_fits(
    spans: &mut Vec<Span<'static>>,
    width: u16,
    segment: String,
    style: Style,
) -> bool {
    let available = usize::from(width).saturating_sub(status_strip_width(spans));
    if segment.chars().count() > available {
        return false;
    }

    spans.push(Span::styled(segment, style));
    true
}

fn status_strip_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

pub(super) fn render_prompt_pane(frame: &mut Frame, app: &AppState, area: Rect, theme: &Theme) {
    if app.replay_mode {
        let surface = theme.surface.panel_elevated;
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.status.disabled))
            .style(Style::default().bg(surface))
            .title(Line::from(Span::styled(
                "Replay archive · read-only",
                Style::default().fg(theme.status.disabled),
            )));
        let paragraph = Paragraph::new(
            "Replay is read-only — prompt editing and submit stay disabled. Inspect the transcript, event log, or diff, then press r to reload.",
        )
        .block(block)
        .style(panel_style(surface, theme.status.disabled))
        .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
        return;
    }

    let is_focused = app.focus == Focus::Prompt;
    let runtime_state = app.runtime_state();
    let composer_disabled = runtime_state.composer_disabled;

    let char_count = app.prompt_buffer.chars().count();
    let composer_lines = composer_input_height(&app.prompt_buffer, area.width);
    let redesign_active = app.transcript_first_shell_redesign_active();
    let title = if composer_disabled {
        format!("Composer · disabled · {}", runtime_state.kind.label())
    } else if redesign_active {
        let composer_mode = if char_count == 0 { "ready" } else { "draft" };
        format!(
            "Composer · {composer_mode} · {} {} · {} chars",
            composer_lines,
            line_label(composer_lines),
            char_count
        )
    } else {
        format!(
            "Composer · {} {} · {} chars",
            composer_lines,
            line_label(composer_lines),
            char_count
        )
    };

    let surface = if redesign_active {
        theme.surface.shell
    } else {
        theme.surface.panel_elevated
    };
    let border_color = if composer_disabled {
        theme.status.disabled
    } else if is_focused {
        theme.border.focus
    } else {
        theme.border.subtle
    };
    let title_color = if composer_disabled {
        theme.status.disabled
    } else if is_focused {
        theme.text.primary
    } else {
        theme.text.secondary
    };
    let block = Block::default()
        .borders(if redesign_active {
            Borders::TOP | Borders::BOTTOM
        } else {
            Borders::ALL
        })
        .border_style(Style::default().fg(border_color))
        .style(Style::default().bg(surface))
        .title(Line::from(Span::styled(
            title,
            if redesign_active {
                Style::default()
                    .fg(title_color)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(title_color)
            },
        )));

    let mut text = app.prompt_buffer.clone();
    if is_focused && !composer_disabled {
        let cursor_byte_pos = app
            .prompt_buffer
            .char_indices()
            .nth(app.prompt_cursor)
            .map(|(i, _)| i)
            .unwrap_or(app.prompt_buffer.len());
        text.insert(cursor_byte_pos, '█');
    }

    if redesign_active && !text.is_empty() {
        text = format_composer_text(&text, theme);
    }

    let (text, style) = if text.is_empty() {
        let hint_color = if composer_disabled {
            theme.status.disabled
        } else {
            theme.text.secondary
        };
        (
            runtime_state.composer_hint,
            panel_style(surface, hint_color),
        )
    } else if composer_disabled {
        (text, panel_style(surface, theme.status.disabled))
    } else {
        (text, panel_style(surface, theme.text.primary))
    };

    let paragraph = Paragraph::new(text)
        .block(block)
        .style(style)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

pub(super) fn panel_style(surface: Color, foreground: Color) -> Style {
    Style::default().fg(foreground).bg(surface)
}

pub(super) fn panel_block<'a>(
    theme: &Theme,
    title: impl Into<Line<'a>>,
    is_focused: bool,
    surface: Color,
) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(panel_border_style(theme, is_focused))
        .style(Style::default().bg(surface))
        .title(title)
        .title_style(panel_style(surface, theme.text.secondary))
}

pub(super) fn elevated_card_block<'a>(
    title: impl Into<Line<'a>>,
    surface: Color,
    border: Color,
    title_color: Color,
) -> Block<'a> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(surface))
        .title(title)
        .title_style(panel_style(surface, title_color))
}

fn panel_border_style(theme: &Theme, is_focused: bool) -> Style {
    let border = if is_focused {
        theme.border.focus
    } else {
        theme.border.subtle
    };
    Style::default().fg(border)
}

pub(super) fn request_id_label(request_id: &str) -> Cow<'_, str> {
    if request_id.is_empty() {
        Cow::Borrowed("pending turn")
    } else {
        Cow::Borrowed(request_id)
    }
}

pub(super) fn transcript_label_style(theme: &Theme, is_selected: bool) -> Style {
    let color = if is_selected {
        theme.text.accent
    } else {
        theme.text.primary
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn muted_meta_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

pub(super) fn subdued_payload_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.secondary)
}

pub(super) fn transcript_prefix_style(theme: &Theme) -> Style {
    Style::default().fg(theme.text.tertiary)
}

pub(super) fn status_badge(label: impl Into<String>, color: Color, theme: &Theme) -> Span<'static> {
    Span::styled(
        format!(" {} ", label.into()),
        Style::default()
            .fg(theme.text.inverse)
            .bg(color)
            .add_modifier(Modifier::BOLD),
    )
}

pub(super) fn tool_status_badge(status: ToolCallDisplayStatus, theme: &Theme) -> Span<'static> {
    let (_, color, label, _) = tool_status_tokens(status, theme);
    status_badge(label, color, theme)
}

pub(super) fn tool_detail_label_style(
    label: &str,
    theme: &Theme,
    status: ToolCallDisplayStatus,
) -> Style {
    let color = match label {
        "state" => match status {
            ToolCallDisplayStatus::PendingPermission => theme.status.warning,
            ToolCallDisplayStatus::Queued => theme.text.secondary,
            ToolCallDisplayStatus::Running => theme.text.accent,
            ToolCallDisplayStatus::Succeeded => theme.status.success,
            ToolCallDisplayStatus::Failed => theme.status.error,
        },
        "result" => theme.status.success,
        "error" => theme.status.error,
        _ => theme.text.secondary,
    };
    Style::default().fg(color).add_modifier(Modifier::BOLD)
}

pub(super) fn tool_state_summary(tool_call: &crate::app::ToolCallEntry) -> Option<&'static str> {
    match tool_call.status {
        ToolCallDisplayStatus::PendingPermission => Some("awaiting approval before execution"),
        ToolCallDisplayStatus::Queued => Some("waiting for execution"),
        ToolCallDisplayStatus::Running => Some("running…"),
        ToolCallDisplayStatus::Succeeded if tool_call.truncated_output.is_none() => {
            Some("completed without output")
        }
        ToolCallDisplayStatus::Failed if tool_call.truncated_output.is_none() => {
            Some("failed without error payload")
        }
        _ => None,
    }
}

pub(super) fn tool_footer_summary(tool_call: &crate::app::ToolCallEntry) -> String {
    let mut parts = vec![format!("call {}", tool_call.tool_call_id)];
    if !tool_call.permissions.is_empty() {
        let count = tool_call.permissions.len();
        parts.push(format!(
            "{count} permission{}",
            if count == 1 { "" } else { "s" }
        ));
    }
    parts.join(" · ")
}

fn tool_status_summary(app: &AppState) -> Option<(String, Color)> {
    let activity = app.activities.get(app.selected_activity_index)?;
    let tool_calls = &activity.tool_calls;
    if tool_calls.is_empty() {
        return None;
    }

    if tool_calls.len() == 1 {
        let tool_call = &tool_calls[0];
        let (_, color, label, _) = tool_status_tokens(tool_call.status, app.theme());
        return Some((format!("tool {} {label}", tool_call.tool_id), color));
    }

    let mut pending = 0usize;
    let mut queued = 0usize;
    let mut running = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for tool_call in tool_calls {
        match tool_call.status {
            ToolCallDisplayStatus::PendingPermission => pending += 1,
            ToolCallDisplayStatus::Queued => queued += 1,
            ToolCallDisplayStatus::Running => running += 1,
            ToolCallDisplayStatus::Succeeded => succeeded += 1,
            ToolCallDisplayStatus::Failed => failed += 1,
        }
    }

    let mut segments = vec!["tools".to_string()];
    if running > 0 {
        segments.push(format!("{running} running"));
    }
    if pending > 0 {
        segments.push(format!("{pending} approval"));
    }
    if queued > 0 {
        segments.push(format!("{queued} queued"));
    }
    if failed > 0 {
        segments.push(format!("{failed} failed"));
    }
    if succeeded > 0 {
        segments.push(format!("{succeeded} done"));
    }

    let color = if failed > 0 {
        app.theme().status.error
    } else if pending > 0 {
        app.theme().status.warning
    } else if running > 0 {
        app.theme().text.accent
    } else if queued > 0 {
        app.theme().text.secondary
    } else {
        app.theme().status.success
    };

    Some((segments.join(" · "), color))
}

pub(super) fn compact_inline_payload(payload: &str, max_chars: usize) -> Option<String> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }

    let collapsed = match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(value) => compact_inline_json_value(&value),
        Err(_) => trimmed.split_whitespace().collect::<Vec<_>>().join(" "),
    };
    if collapsed.chars().count() <= max_chars {
        return Some(collapsed);
    }

    let truncated = collapsed
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    Some(format!("{truncated}…"))
}

fn compact_inline_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                return "{}".to_string();
            }

            let mut parts = Vec::new();
            for (key, value) in map.iter().take(4) {
                parts.push(format!("{key}={}", compact_inline_json_leaf(value)));
            }
            if map.len() > 4 {
                parts.push("…".to_string());
            }
            parts.join(", ")
        }
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                return "[]".to_string();
            }

            let mut parts = items
                .iter()
                .take(4)
                .map(compact_inline_json_leaf)
                .collect::<Vec<_>>();
            if items.len() > 4 {
                parts.push("…".to_string());
            }
            format!("[{}]", parts.join(", "))
        }
        _ => compact_inline_json_leaf(value),
    }
}

fn compact_inline_json_leaf(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.split_whitespace().collect::<Vec<_>>().join(" "),
        serde_json::Value::Number(number) => number.to_string(),
        serde_json::Value::Bool(flag) => flag.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(items) => format!(
            "[{} item{}]",
            items.len(),
            if items.len() == 1 { "" } else { "s" }
        ),
        serde_json::Value::Object(fields) => format!(
            "{{{} field{}}}",
            fields.len(),
            if fields.len() == 1 { "" } else { "s" }
        ),
    }
}

pub(super) fn tool_status_tokens(
    status: ToolCallDisplayStatus,
    theme: &Theme,
) -> (&'static str, Color, &'static str, bool) {
    match status {
        ToolCallDisplayStatus::PendingPermission => (
            theme.live_shell.glyphs.pending_permission,
            theme.status.warning,
            "pending permission",
            false,
        ),
        ToolCallDisplayStatus::Queued => (
            theme.live_shell.glyphs.queued,
            theme.text.secondary,
            "queued",
            false,
        ),
        ToolCallDisplayStatus::Running => (
            theme.live_shell.glyphs.running,
            theme.text.accent,
            "running",
            false,
        ),
        ToolCallDisplayStatus::Succeeded => (
            theme.live_shell.glyphs.succeeded,
            theme.status.success,
            "succeeded",
            true,
        ),
        ToolCallDisplayStatus::Failed => (
            theme.live_shell.glyphs.failed,
            theme.status.error,
            "failed",
            true,
        ),
    }
}

pub(super) fn line_label(count: u16) -> &'static str {
    if count == 1 {
        "line"
    } else {
        "lines"
    }
}

pub(super) fn runtime_state_color(kind: RuntimeStateKind, theme: &Theme) -> Color {
    match kind {
        RuntimeStateKind::Ready => theme.status.info,
        RuntimeStateKind::Success => theme.status.success,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming => theme.status.info,
        RuntimeStateKind::Cancelled
        | RuntimeStateKind::PermissionBlocked
        | RuntimeStateKind::PermissionPending
        | RuntimeStateKind::Degraded => theme.status.warning,
        RuntimeStateKind::Failure | RuntimeStateKind::Disconnected => theme.status.error,
    }
}

pub(super) fn truncate_plain_text(text: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }

    let text_width = text.chars().count();
    if text_width <= max_width {
        return text.to_string();
    }

    if max_width == 1 {
        return "…".to_string();
    }

    let truncated = text
        .chars()
        .take(max_width.saturating_sub(1))
        .collect::<String>();
    format!("{truncated}…")
}

fn footer_context(app: &AppState, theme: &Theme) -> (&'static str, Color) {
    if app.replay_mode {
        return ("replay", theme.border.strong);
    }
    if app.startup_shell_visible() {
        return ("launcher", theme.border.strong);
    }
    if app.post_run_handoff_visible() {
        return ("next action", theme.status.warning);
    }

    match app.active_tab {
        Tab::Events => ("events", theme.border.strong),
        Tab::Diff => ("diff", theme.border.strong),
        Tab::Help => ("help", theme.border.strong),
        Tab::Run | Tab::Details => {
            if app.details_drawer_open() {
                match app.focus {
                    Focus::Prompt => ("prompt", theme.border.focus),
                    Focus::List => ("orchestration", theme.border.focus),
                    Focus::Details => ("inspector", theme.border.focus),
                }
            } else if app.focus == Focus::Prompt {
                ("prompt", theme.border.focus)
            } else {
                ("conversation", theme.border.strong)
            }
        }
    }
}

fn primary_shell_context_width(theme: &Theme) -> u16 {
    theme.live_shell.breakpoints.primary.width
}

fn status_context(app: &AppState, theme: &Theme, state: RuntimeStateKind) -> (&'static str, Color) {
    if app.startup_shell_visible() {
        return ("launcher", theme.border.strong);
    }
    if app.post_run_handoff_visible() {
        return ("next action", theme.status.warning);
    }
    if app.replay_mode {
        return ("replay", theme.border.strong);
    }
    if app.active_tab == Tab::Details && app.details_drawer_open() {
        return ("details", theme.border.focus);
    }
    if matches!(
        state,
        RuntimeStateKind::Sending | RuntimeStateKind::Streaming
    ) {
        return ("live", theme.text.accent);
    }
    ("live", theme.border.strong)
}

fn format_composer_text(text: &str, theme: &Theme) -> String {
    let padding = " ".repeat(theme.live_shell.rhythm.composer_padding_x as usize);
    let first_prefix = format!(
        "{padding}{} ",
        theme.live_shell.transcript_glyphs.user_marker
    );
    let continuation_prefix = format!("{padding}  ");

    text.split('\n')
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                format!("{first_prefix}{line}")
            } else {
                format!("{continuation_prefix}{line}")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
