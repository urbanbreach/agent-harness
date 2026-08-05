// allow: SIZE_OK — TUI rendering (indivisible view model)
use super::*;
use crate::UnwrapOrAbort;
use ratatui::widgets::BorderType;

pub(super) const COMPOSER_PROMPT_GLYPH: &str = "❯";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposerViewport {
    pub(super) lines: Vec<String>,
    pub(super) line_starts: Vec<usize>,
    pub(super) cursor: Option<(usize, usize)>,
}

#[derive(Debug, Clone, Copy)]
struct ComposerVisualChar {
    index: usize,
    ch: char,
    width: usize,
}

type ComposerVisualLines = (Vec<(String, usize)>, Option<(usize, usize)>);

pub(super) fn render_document_composer_content(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    let bordered_composer = matches!(
        context.dock.variant,
        crate::view_model::ControlDockVariant::Startup
            | crate::view_model::ControlDockVariant::Live
    ) && app.active_permission_view().is_none()
        && app.transcript_pending_permissions().is_empty();
    if bordered_composer {
        render_bordered_composer(frame, app, area, theme, context);
        return;
    }
    let surface = control_dock_surface(theme, context.dock.variant);
    let composer_surface = composer_input_surface(theme);
    let prompt_area = area;
    frame.render_widget(Block::default().style(Style::default().bg(surface)), area);

    if prompt_area.width == 0 || prompt_area.height == 0 {
        return;
    }

    let main_columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .split(prompt_area);
    let rail_area = main_columns[0];
    let body_area = main_columns[1];

    let shell_rows = if body_area.height > 1 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(body_area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(0)])
            .split(body_area)
    };
    let composer_body_area = shell_rows[0];

    frame.render_widget(
        Block::default().style(Style::default().bg(composer_surface)),
        composer_body_area,
    );

    if body_area.width == 0 || body_area.height == 0 {
        return;
    }

    let body_inner = inset_rect(
        composer_body_area,
        theme
            .live_shell
            .rhythm
            .composer_padding_x
            .min(composer_body_area.width.saturating_sub(1)),
        0,
    );
    if body_inner.width == 0 || body_inner.height == 0 {
        return;
    }

    let metadata_height = u16::from(body_inner.height >= 2);
    let metadata_gap = u16::from(metadata_height > 0 && body_inner.height >= 4);
    let top_padding = u16::from(
        body_inner.height
            >= context
                .composer_lines
                .saturating_add(metadata_height)
                .saturating_add(metadata_gap)
                .saturating_add(1),
    );
    let available_input_height = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height)
        .max(1);
    let input_height = context
        .composer_lines
        .clamp(1, available_input_height)
        .max(1);
    let trailing_fill = body_inner
        .height
        .saturating_sub(top_padding)
        .saturating_sub(input_height)
        .saturating_sub(metadata_gap)
        .saturating_sub(metadata_height);

    let pre_input_fill = 0;
    let post_metadata_fill = trailing_fill;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_padding.saturating_add(pre_input_fill)),
            Constraint::Length(input_height),
            Constraint::Length(metadata_gap),
            Constraint::Length(metadata_height),
            Constraint::Length(post_metadata_fill),
        ])
        .split(body_inner);
    let input_area = rows[1];
    let input_width = usize::from(input_area.width);
    let composer_text = app.composer_render_text();
    let placeholder_visible = composer_text.is_empty()
        && matches!(
            context.dock.variant,
            crate::view_model::ControlDockVariant::Startup
        );
    let shell_mode_active = app.shell_mode() && !context.dock.composer_disabled;
    let body = if placeholder_visible {
        context.dock.composer_body.as_str()
    } else if shell_mode_active && composer_text.is_empty() {
        "run a shell command…"
    } else {
        composer_text.as_str()
    };
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if placeholder_visible {
        Color::Reset
    } else if shell_mode_active {
        theme.status.warning
    } else {
        Color::Reset
    };
    let glyph_style = if context.dock.composer_disabled {
        Style::default().fg(theme.status.disabled).bg(surface)
    } else if shell_mode_active {
        Style::default().fg(theme.status.warning).bg(surface)
    } else {
        Style::default().fg(Color::Reset).bg(surface)
    };

    if rail_area.height > 0 && rail_area.width > 0 {
        let height = usize::from(rail_area.height);
        let mut rail_lines = Vec::with_capacity(height.max(1));
        if height > 0 {
            rail_lines.push(Line::from(Span::styled(COMPOSER_PROMPT_GLYPH, glyph_style)));
            rail_lines.extend(
                std::iter::repeat_with(|| Line::from(Span::styled(" ", glyph_style)))
                    .take(height.saturating_sub(1)),
            );
        }
        frame.render_widget(
            Paragraph::new(rail_lines).style(Style::default().bg(surface)),
            rail_area,
        );
    }

    let show_cursor = !context.dock.composer_disabled
        && !footer_suppressed_by_overlay(app)
        && (placeholder_visible || context.dock.composer_focused);
    let mut viewport = composer_viewport(
        body,
        input_width,
        usize::from(input_area.height.max(1)),
        show_cursor.then_some(if placeholder_visible {
            0
        } else {
            app.composer_render_cursor()
        }),
    );
    if !show_cursor {
        viewport.cursor = None;
    }
    let base_style = Style::default().fg(body_color).bg(composer_surface);
    let tag_style = Style::default()
        .fg(theme.status.warning)
        .bg(composer_surface)
        .add_modifier(Modifier::BOLD);
    let body_lines = viewport
        .lines
        .iter()
        .zip(viewport.line_starts.iter().copied())
        .map(|(line, start)| {
            if placeholder_visible || context.dock.composer_disabled {
                Line::from(Span::styled(line.clone(), base_style))
            } else {
                composer_line_with_file_tags(
                    line,
                    start,
                    &app.file_mention_tags,
                    base_style,
                    tag_style,
                )
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        input_area,
    );

    if let Some((cursor_row, cursor_col)) = viewport.cursor {
        let cursor_x = input_area
            .x
            .saturating_add(u16::try_from(cursor_col).unwrap_or(u16::MAX));
        let cursor_y = input_area
            .y
            .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX));
        frame.set_cursor_position((cursor_x, cursor_y));
    }

    if metadata_height > 0 && rows[3].width > 0 {
        frame.render_widget(
            Paragraph::new(composer_metadata_line(
                app,
                context.dock,
                context.disclosure_visible,
                usize::from(rows[3].width),
                theme,
                composer_agent_accent(theme, app),
                composer_surface,
            ))
            .style(Style::default().bg(composer_surface)),
            rows[3],
        );
    }
}

fn composer_model_badge(app: &AppState) -> String {
    let model = composer_event_model_id(app).unwrap_or_else(|| app.current_model_base_label());
    let mut parts = Vec::new();
    if let Some(profile) = active_turn_profile_label(app) {
        parts.push(profile);
    }
    if !model.is_empty() && model != "-" && !model.eq_ignore_ascii_case("unknown") {
        parts.push(model.to_string());
    } else if !app.composer.prompt_buffer.is_empty() {
        parts.push("unknown".to_string());
    }
    if let Some(reasoning) = app.current_model_reasoning_label() {
        if !reasoning.is_empty()
            && !parts
                .iter()
                .any(|part| part.eq_ignore_ascii_case(reasoning) || part.contains(reasoning))
        {
            parts.push(reasoning.to_string());
        }
    }
    if app.always_approve_mode() {
        parts.push("always-approve".to_string());
    } else if app.session_mode() == crate::app::SessionMode::Plan {
        parts.push("plan".to_string());
    }
    if app.queued_prompt_count > 0 {
        parts.push(format!("queued {}", app.queued_prompt_count));
    }
    if app.composer.multiline_mode {
        parts.push("multiline".to_string());
    }
    parts.join(" · ")
}

fn render_bordered_composer(
    frame: &mut Frame,
    app: &AppState,
    area: Rect,
    theme: &Theme,
    context: DocumentComposerRenderContext<'_>,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let surface = if context.dock.variant == crate::view_model::ControlDockVariant::Live {
        theme.reference_terminal.canvas
    } else {
        Color::Reset
    };
    let composer_surface = surface;
    let border_fg = if context.dock.variant == crate::view_model::ControlDockVariant::Live {
        live_composer_border_color(theme)
    } else {
        Color::Reset
    };
    let border_style = Style::default().fg(border_fg).bg(surface);
    let composer_view = app.composer_view_model_for_area(area);
    let mut badge = composer_model_badge(app);
    if !composer_view.attachments.is_empty() {
        let labels = composer_view
            .attachments
            .iter()
            .map(|attachment| attachment.label.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        badge.push_str(&format!(" · {labels}"));
    }
    if let Some(completion) = composer_view.completion.as_ref() {
        badge.push_str(&format!(" · {} suggestions", completion.items.len()));
    }
    let content_lines = context.composer_lines.max(1);
    let strip_height = area
        .height
        .min(content_lines.saturating_add(2))
        .max(3.min(area.height).max(1));
    let strip = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: strip_height,
    };
    let mut block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border_style)
        .style(Style::default().bg(surface));
    let (badge_title, badge_style) = if badge.is_empty() {
        ("  ─".to_string(), border_style)
    } else {
        (
            format!(" {badge} ─"),
            Style::default()
                .fg(theme.reference_terminal.secondary)
                .bg(surface),
        )
    };
    block = block.title_bottom(Line::from(Span::styled(badge_title, badge_style)).right_aligned());

    let inner = block.inner(strip);
    frame.render_widget(block, strip);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let shell_mode_active = app.shell_mode() && !context.dock.composer_disabled;
    let composer_text = app.composer_render_text();
    let placeholder_visible = composer_text.is_empty();
    let body = if placeholder_visible {
        ""
    } else {
        composer_text.as_str()
    };
    let body_color = if context.dock.composer_disabled {
        theme.status.disabled
    } else if shell_mode_active {
        theme.status.warning
    } else {
        Color::Reset
    };
    let glyph_style = if context.dock.composer_disabled {
        Style::default()
            .fg(theme.status.disabled)
            .bg(composer_surface)
    } else if shell_mode_active {
        Style::default()
            .fg(theme.status.warning)
            .bg(composer_surface)
    } else {
        Style::default().fg(Color::Reset).bg(composer_surface)
    };

    let glyph_prefix = format!(" {COMPOSER_PROMPT_GLYPH} ");
    let glyph_cols = display_width(&glyph_prefix);
    let draft_width = usize::from(inner.width).saturating_sub(glyph_cols).max(1);
    let max_visible = usize::from(inner.height.min(content_lines).max(1));
    let show_cursor = !context.dock.composer_disabled
        && !footer_suppressed_by_overlay(app)
        && (placeholder_visible || context.dock.composer_focused);
    let mut viewport = composer_viewport(
        body,
        draft_width,
        max_visible,
        show_cursor.then_some(if placeholder_visible {
            0
        } else {
            app.composer_render_cursor()
        }),
    );
    if !show_cursor {
        viewport.cursor = None;
    }

    let base_style = if body_color == Color::Reset {
        Style::default().bg(composer_surface)
    } else {
        Style::default().fg(body_color).bg(composer_surface)
    };
    let body_lines = viewport
        .lines
        .iter()
        .enumerate()
        .map(|(row, line)| {
            if row == 0 {
                Line::from(vec![
                    Span::styled(glyph_prefix.clone(), glyph_style),
                    Span::styled(line.clone(), base_style),
                ])
            } else {
                Line::from(vec![
                    Span::styled(" ".repeat(glyph_cols), base_style),
                    Span::styled(line.clone(), base_style),
                ])
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(body_lines).style(Style::default().bg(composer_surface)),
        inner,
    );

    if let Some((cursor_row, cursor_col)) = viewport.cursor {
        let cursor_x = inner
            .x
            .saturating_add(
                u16::try_from(glyph_cols.saturating_add(cursor_col)).unwrap_or(u16::MAX),
            )
            .min(inner.x.saturating_add(inner.width.saturating_sub(1)));
        let cursor_y = inner
            .y
            .saturating_add(u16::try_from(cursor_row).unwrap_or(u16::MAX))
            .min(inner.y.saturating_add(inner.height.saturating_sub(1)));
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

const fn live_composer_border_color(theme: &Theme) -> Color {
    theme.reference_terminal.prompt_border_active
}

#[cfg(test)]
mod active_thinking_color_tests {
    use super::*;

    #[test]
    fn live_composer_border_matches_the_groknight_active_prompt() {
        let theme = Theme::harness_chat();

        assert_eq!(
            live_composer_border_color(&theme),
            Color::Rgb(0x50, 0x50, 0x58)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposerMetadataTone {
    Accent,
    AgentAccent,
    Primary,
    Secondary,
}

fn composer_metadata_line(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    _disclosure_visible: bool,
    max_width: usize,
    theme: &Theme,
    agent_accent: Color,
    surface: Color,
) -> Line<'static> {
    let candidates = composer_metadata_candidates(app, dock);
    let segments = candidates
        .iter()
        .find(|segments| composer_metadata_segments_width(segments) <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            vec![(
                truncate_plain_text(&composer_metadata_text(app, dock, max_width), max_width),
                ComposerMetadataTone::Secondary,
            )]
        });

    Line::from(
        segments
            .into_iter()
            .map(|(text, tone)| {
                Span::styled(
                    text,
                    Style::default()
                        .fg(composer_metadata_color(tone, theme, agent_accent))
                        .bg(surface),
                )
            })
            .collect::<Vec<_>>(),
    )
}

fn composer_metadata_color(
    tone: ComposerMetadataTone,
    theme: &Theme,
    agent_accent: Color,
) -> Color {
    match tone {
        ComposerMetadataTone::Accent => composer_input_accent(theme),
        ComposerMetadataTone::AgentAccent => agent_accent,
        ComposerMetadataTone::Primary => composer_input_text(theme),
        ComposerMetadataTone::Secondary => composer_input_muted(theme),
    }
}

fn composer_metadata_segments_width(segments: &[(String, ComposerMetadataTone)]) -> usize {
    segments
        .iter()
        .map(|(text, _)| text.chars().count())
        .sum::<usize>()
}

pub(super) fn composer_metadata_candidates(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
) -> Vec<Vec<(String, ComposerMetadataTone)>> {
    let profile = active_turn_profile_label(app).or_else(|| app.current_agent_label());
    let model = composer_event_model_id(app)
        .map(str::to_owned)
        .unwrap_or_else(|| app.current_model_base_label().to_string());
    let source = app.current_source_label();
    let tail = app
        .current_model_reasoning_label()
        .map(str::to_string)
        .or_else(|| {
            (has_trimmed_content(&dock.runtime_badge)
                && dock.runtime_kind != RuntimeStateKind::Ready
                && dock.runtime_kind != RuntimeStateKind::Success)
                .then(|| dock.runtime_badge.to_ascii_lowercase())
        });
    let queue_indicator =
        (app.queued_prompt_count > 0).then(|| format!("queued {}", app.queued_prompt_count));

    let mut full = Vec::new();
    if let Some(profile) = profile.clone() {
        full.push((profile, ComposerMetadataTone::AgentAccent));
    }
    if !model.is_empty() && model != "-" {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((model.clone(), ComposerMetadataTone::Primary));
    }
    if let Some(source) = source.clone() {
        if !full.is_empty() {
            full.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((source, ComposerMetadataTone::Secondary));
    }
    if let Some(tail) = tail.as_ref() {
        if !full.is_empty() {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((tail.clone(), ComposerMetadataTone::Accent));
    }
    if let Some(queue) = queue_indicator.as_ref() {
        if !full.is_empty() {
            full.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        full.push((queue.clone(), ComposerMetadataTone::Accent));
    }

    let mut compact = Vec::new();
    if let Some(profile) = profile.as_ref() {
        compact.push((profile.clone(), ComposerMetadataTone::AgentAccent));
    }
    if !model.is_empty() && model != "-" {
        if !compact.is_empty() {
            compact.push((" ".to_string(), ComposerMetadataTone::Secondary));
        }
        compact.push((model, ComposerMetadataTone::Primary));
    }
    if let Some(queue) = queue_indicator.as_ref() {
        if !compact.is_empty() {
            compact.push((" · ".to_string(), ComposerMetadataTone::Secondary));
        }
        compact.push((queue.clone(), ComposerMetadataTone::Accent));
    }

    let queue_only = queue_indicator
        .as_ref()
        .map(|queue| vec![(queue.clone(), ComposerMetadataTone::Accent)]);

    let mut candidates = vec![full, compact];
    if let Some(queue_candidate) = queue_only {
        candidates.push(queue_candidate);
    }
    candidates.push(
        source
            .map(|source| vec![(source, ComposerMetadataTone::Secondary)])
            .or_else(|| {
                profile
                    .as_ref()
                    .map(|profile| vec![(profile.clone(), ComposerMetadataTone::AgentAccent)])
            })
            .unwrap_or_default(),
    );
    candidates.push(vec![(
        dock.primary_summary.clone(),
        ComposerMetadataTone::Secondary,
    )]);
    candidates
}

fn composer_event_model_id(app: &AppState) -> Option<&str> {
    app.events.iter().rev().find_map(|event| match &event.payload {
        harness_core::event::EventV1::ProviderRequestStarted(payload)
            if !payload.model_id.trim().is_empty() =>
        {
            Some(payload.model_id.as_str())
        }
        _ => None,
    })
}

fn active_turn_profile_label(app: &AppState) -> Option<String> {
    let hidden_child_request_ids = app.hidden_delegated_child_request_ids_in_current_view();
    let activity =
        app.activities
            .iter()
            .rev()
        .filter(|activity| {
            activity.request_id.is_empty()
                || !hidden_child_request_ids.contains(activity.request_id.as_str())
        })
            .find(|activity| activity.status == ActivityStatus::Streaming)
            .or_else(|| {
                app.activities.iter().rev().find(|activity| {
                    activity.request_id.is_empty()
                        || !hidden_child_request_ids.contains(activity.request_id.as_str())
                })
            })?;
    if !matches!(
        activity.status,
        ActivityStatus::Streaming | ActivityStatus::Queued
    ) {
        return None;
    }
    let profile = activity.profile_label.trim();
    if profile.is_empty() || profile.eq_ignore_ascii_case("unknown") {
        return None;
    }
    Some(crate::app::humanize_profile_label(profile))
}

fn composer_metadata_text(
    app: &AppState,
    dock: &crate::view_model::ControlDockViewModel,
    max_width: usize,
) -> String {
    if max_width == 0 {
        return String::new();
    }

    let profile = active_turn_profile_label(app)
        .or_else(|| app.current_agent_label())
        .unwrap_or_else(|| app.active_profile().to_string());

    best_fit_text(
        &[
            Some(format!("{profile} {}", app.current_model_label())),
            app.launch_mode_label().map(str::to_string),
            Some(dock.primary_summary.clone()),
            Some(app.current_model_label().to_string()),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>(),
        max_width,
    )
}

fn best_fit_text(options: &[String], max_width: usize) -> String {
    options
        .iter()
        .find(|option| option.chars().count() <= max_width)
        .cloned()
        .unwrap_or_else(|| {
            truncate_plain_text(options.first().map(String::as_str).unwrap_or(""), max_width)
        })
}

pub(super) fn composer_viewport(
    text: &str,
    width: usize,
    max_lines: usize,
    cursor_char_index: Option<usize>,
) -> ComposerViewport {
    if max_lines == 0 {
        return ComposerViewport {
            lines: Vec::new(),
            line_starts: Vec::new(),
            cursor: None,
        };
    }

    let (wrapped, cursor) = composer_visual_lines(text, width, cursor_char_index);

    let total_lines = wrapped.len();
    let visible_count = max_lines.min(total_lines).max(1);
    let anchor_row = cursor
        .map(|(row, _)| row)
        .unwrap_or(total_lines.saturating_sub(1));
    let start_row = anchor_row
        .saturating_add(1)
        .saturating_sub(visible_count)
        .min(total_lines.saturating_sub(visible_count));
    let end_row = start_row.saturating_add(visible_count).min(total_lines);

    ComposerViewport {
        lines: wrapped[start_row..end_row]
            .iter()
            .map(|(line, _)| line.clone())
            .collect(),
        line_starts: wrapped[start_row..end_row]
            .iter()
            .map(|(_, start)| *start)
            .collect(),
        cursor: cursor.and_then(|(row, column)| {
            (row >= start_row && row < end_row).then_some((row - start_row, column))
        }),
    }
}

fn composer_visual_lines(
    text: &str,
    width: usize,
    cursor_char_index: Option<usize>,
) -> ComposerVisualLines {
    let width = width.max(1);
    let mut lines = Vec::new();
    let mut cursor = None;
    let chars = text
        .chars()
        .enumerate()
        .map(|(index, ch)| ComposerVisualChar {
            index,
            ch,
            width: display_width(&ch.to_string()).max(1),
        })
        .collect::<Vec<_>>();

    let mut segment_start = 0usize;
    let mut fallback_start = 0usize;
    for position in 0..=chars.len() {
        let hard_break = position == chars.len() || chars[position].ch == '\n';
        if !hard_break {
            continue;
        }

        wrap_composer_visual_segment(
            &chars[segment_start..position],
            fallback_start,
            width,
            cursor_char_index,
            &mut cursor,
            &mut lines,
        );

        if position < chars.len() {
            if cursor_char_index == Some(chars[position].index) {
                let row = lines.len().saturating_sub(1);
                let column = lines
                    .last()
                    .map(|(line, _)| display_width(line))
                    .unwrap_or(0);
                cursor = Some((row, column));
            }
            fallback_start = chars[position].index + 1;
            segment_start = position + 1;
        }
    }

    if cursor_char_index == Some(text.chars().count()) {
        let row = lines.len().saturating_sub(1);
        let column = lines
            .last()
            .map(|(line, _)| display_width(line))
            .unwrap_or(0);
        cursor = Some((row, column));
    }

    (lines, cursor)
}

fn wrap_composer_visual_segment(
    chars: &[ComposerVisualChar],
    fallback_start: usize,
    width: usize,
    cursor_char_index: Option<usize>,
    cursor: &mut Option<(usize, usize)>,
    lines: &mut Vec<(String, usize)>,
) {
    if chars.is_empty() {
        emit_composer_visual_line(chars, fallback_start, cursor_char_index, cursor, lines);
        return;
    }

    let mut start = 0usize;
    while start < chars.len() {
        let fit_end = composer_fit_end(chars, start, width);
        if fit_end >= chars.len() {
            emit_composer_visual_line(
                &chars[start..],
                chars[start].index,
                cursor_char_index,
                cursor,
                lines,
            );
            break;
        }

        if let Some(break_at) = chars[start..fit_end]
            .iter()
            .rposition(|visual_char| visual_char.ch.is_whitespace())
            .map(|offset| start + offset)
            .filter(|break_at| *break_at > start)
        {
            let end = break_at + 1;
            emit_composer_visual_line(
                &chars[start..end],
                chars[start].index,
                cursor_char_index,
                cursor,
                lines,
            );
            start = end;
            continue;
        }

        if chars[fit_end].ch.is_whitespace() {
            emit_composer_visual_line(
                &chars[start..fit_end],
                chars[start].index,
                cursor_char_index,
                cursor,
                lines,
            );
            if cursor_char_index == Some(chars[fit_end].index) {
                let row = lines.len().saturating_sub(1);
                let column = lines
                    .last()
                    .map(|(line, _)| display_width(line))
                    .unwrap_or(0);
                *cursor = Some((row, column));
            }
            start = fit_end + 1;
            continue;
        }

        let end = fit_end.max(start + 1);
        emit_composer_visual_line(
            &chars[start..end],
            chars[start].index,
            cursor_char_index,
            cursor,
            lines,
        );
        start = end;
    }
}

fn composer_fit_end(chars: &[ComposerVisualChar], start: usize, width: usize) -> usize {
    let mut used = 0usize;
    for (position, visual_char) in chars.iter().enumerate().skip(start) {
        if position > start && used.saturating_add(visual_char.width) > width {
            return position;
        }
        used = used.saturating_add(visual_char.width);
    }
    chars.len()
}

fn emit_composer_visual_line(
    chars: &[ComposerVisualChar],
    fallback_start: usize,
    cursor_char_index: Option<usize>,
    cursor: &mut Option<(usize, usize)>,
    lines: &mut Vec<(String, usize)>,
) {
    let row = lines.len();
    let line_start = chars
        .first()
        .map(|visual_char| visual_char.index)
        .unwrap_or(fallback_start);
    if let Some(cursor_index) = cursor_char_index {
        if let Some(last) = chars.last() {
            let line_end = last.index + 1;
            if cursor_index >= line_start && cursor_index < line_end {
                let column = chars
                    .iter()
                    .take_while(|visual_char| visual_char.index < cursor_index)
                    .map(|visual_char| visual_char.width)
                    .sum();
                *cursor = Some((row, column));
            }
        } else if cursor_index == line_start {
            *cursor = Some((row, 0));
        }
    }

    lines.push((
        chars.iter().map(|visual_char| visual_char.ch).collect(),
        line_start,
    ));
}

pub(super) fn composer_line_with_file_tags(
    line: &str,
    line_start: usize,
    tags: &[crate::app::FileMentionTag],
    base_style: Style,
    tag_style: Style,
) -> Line<'static> {
    if line.is_empty() {
        return Line::from(Span::styled(String::new(), base_style));
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = None;
    for (offset, ch) in line.chars().enumerate() {
        let char_index = line_start + offset;
        let style = if tags
            .iter()
            .any(|tag| char_index >= tag.start && char_index < tag.end)
        {
            tag_style
        } else {
            base_style
        };
        if current_style == Some(style) {
            current.push(ch);
        } else {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    current_style.unwrap_or_abort(),
                ));
            }
            current_style = Some(style);
            current.push(ch);
        }
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, current_style.unwrap_or(base_style)));
    }
    Line::from(spans)
}
