// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::ui_transcript_surface::TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH;
use super::ui_transcript_tool_render::{
    append_assistant_error_box, append_tool_call_section_lines, shell_tool_uses_harness_bash_card,
    tool_call_is_todo,
};
use super::*;

pub(super) fn build_transcript_render_surfaces(
    section: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    build_turn_render_surfaces(section, theme, width, base_surface)
}

fn build_turn_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    let mut surfaces = Vec::with_capacity(3);
    if let Some(user_message) = turn.user_message.as_ref() {
        surfaces.push(build_user_render_surface(
            turn,
            user_message,
            theme,
            width,
            base_surface,
        ));
    }
    surfaces.extend(build_assistant_render_surfaces(
        turn,
        theme,
        width,
        base_surface,
    ));
    surfaces
}

fn build_user_render_surface(
    turn: &TranscriptTurnSection,
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> TranscriptRenderSurface {
    let surface = base_surface;
    let render_width = transcript_surface_render_width(width, TranscriptRenderSurfaceKind::User);
    let content_width = transcript_surface_content_width(render_width, false);
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let body_style = Style::default().fg(theme.text.primary);
    let mut lines = vec![user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        body_style,
        surface,
    )];
    append_user_surface_text_block(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_USER_BODY_PREFIX,
        content_width,
        surface,
    );
    if user_msg.queued {
        lines.push(user_surface_line(
            TRANSCRIPT_USER_BODY_PREFIX,
            vec![Span::styled(
                " QUEUED ".to_string(),
                Style::default()
                    .fg(selected_foreground_for_badge(agent_accent, theme))
                    .bg(agent_accent)
                    .add_modifier(Modifier::BOLD),
            )],
            Style::default().fg(theme.text.secondary),
            surface,
        ));
    }
    lines.push(user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        body_style,
        surface,
    ));

    if lines.len() > 1 {
        let marker_prefix =
            format!("{} ", theme.live_shell.transcript_glyphs.user_marker);
        lines[1].spans[0] =
            surface_span(marker_prefix, Style::default().fg(theme.text.primary), surface);
        if let Some(clock) = user_msg.wall_clock.as_deref() {
            append_user_row_wall_clock(&mut lines[1], clock, content_width, theme, surface);
        }
    }

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::User,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: agent_accent,
        surface,
        lines,
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
    }
}

fn append_user_row_wall_clock(
    line: &mut Line<'static>,
    clock: &str,
    content_width: u16,
    theme: &Theme,
    surface: Color,
) {
    let used = line
        .spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum::<usize>();
    let clock_width = display_width(clock);
    // content_width is the full user-surface content band (includes prefix).
    let target = usize::from(content_width);
    if clock_width == 0 || used.saturating_add(clock_width) > target {
        return;
    }
    let pad = target.saturating_sub(used).saturating_sub(clock_width);
    if pad > 0 {
        line.spans
            .push(surface_span(" ".repeat(pad), Style::default(), surface));
    }
    line.spans.push(surface_span(
        clock.to_string(),
        Style::default().fg(theme.text.secondary),
        surface,
    ));
}

fn right_aligned_wall_clock_line(clock: &str, content_width: u16, theme: &Theme) -> Line<'static> {
    let mut line = Line::default();
    let clock_width = display_width(clock);
    let target = usize::from(content_width);
    if clock_width == 0 || clock_width > target {
        return line;
    }
    let pad = target.saturating_sub(clock_width);
    if pad > 0 {
        line.spans
            .push(Span::raw(" ".repeat(pad)));
    }
    line.spans.push(Span::styled(
        clock.to_string(),
        Style::default().fg(theme.text.secondary),
    ));
    line
}

fn build_assistant_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<TranscriptRenderSurface> {
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let (assistant_icon, assistant_color, assistant_status) = match turn.header.status {
        ActivityStatus::Queued => ("◇", agent_accent, "queued"),
        ActivityStatus::Streaming => (
            transcript_streaming_spinner_frame(turn.animation_phase),
            agent_accent,
            "active",
        ),
        ActivityStatus::Done => ("", agent_accent, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };

    let mut surfaces = Vec::new();
    let footer_target = assistant_footer_target_index(turn);
    let mut index = 0;

    while index < turn.assistant_parts.len() {
        let part = &turn.assistant_parts[index];
        let previous = index
            .checked_sub(1)
            .and_then(|prev| turn.assistant_parts.get(prev));
        let group_len = match part {
            TranscriptAssistantPart::ToolCall(_) => {
                context_tool_group_len(&turn.assistant_parts[index..])
            }
            _ => 0,
        };

        if group_len > 1
            && context_tool_group_complete(&turn.assistant_parts[index..index + group_len])
        {
            let tool_calls = turn.assistant_parts[index..index + group_len]
                .iter()
                .filter_map(|part| match part {
                    TranscriptAssistantPart::ToolCall(tool_call) => Some(tool_call.as_ref()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            surfaces.push(build_context_tool_group_render_surface(
                turn,
                &tool_calls,
                theme,
                width,
                base_surface,
                footer_target
                    .map(|target| target >= index && target < index + group_len)
                    .unwrap_or(false),
                assistant_icon,
                assistant_color,
                assistant_status,
            ));
            index += group_len;
            continue;
        }

        let append_footer = footer_target == Some(index);
        surfaces.push(build_assistant_part_render_surface(
            turn,
            part,
            theme,
            width,
            base_surface,
            assistant_part_needs_leading_gap(previous, part),
            append_footer,
            assistant_icon,
            assistant_color,
            assistant_status,
        ));
        index += 1;
    }

    if footer_target == Some(turn.assistant_parts.len()) {
        surfaces.push(build_footer_only_render_surface(
            turn,
            theme,
            base_surface,
            width,
            assistant_icon,
            assistant_color,
            assistant_status,
        ));
    }
    // Grok question freeze paints no ❙ while the dock is open (Waiting on answers).
    let paint_selected =
        turn.header.is_selected && waiting_on_answers_label(turn).is_none();
    apply_preferred_selected_rail(&mut surfaces, paint_selected);
    surfaces
}

fn apply_preferred_selected_rail(surfaces: &mut [TranscriptRenderSurface], is_selected: bool) {
    for surface in surfaces.iter_mut() {
        surface.selected_rail = false;
    }
    if !is_selected {
        return;
    }
    let preferred = surfaces
        .iter()
        .rposition(|surface| {
            matches!(
                surface.kind,
                TranscriptRenderSurfaceKind::AssistantTool
                    | TranscriptRenderSurfaceKind::AssistantCommandTool
            )
        })
        .or_else(|| {
            surfaces.iter().position(|surface| {
                surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning
            })
        });
    if let Some(idx) = preferred {
        surfaces[idx].selected_rail = true;
    }
}

fn assistant_footer_target_index(turn: &TranscriptTurnSection) -> Option<usize> {
    if !turn.show_footer {
        return None;
    }
    if turn.assistant_parts.is_empty() {
        return (turn.user_message.is_some()
            || activity_status_supports_footer_only(turn.header.status))
        .then_some(0);
    }

    Some(turn.assistant_parts.len() - 1)
}

fn assistant_part_needs_leading_gap(
    previous: Option<&TranscriptAssistantPart>,
    current: &TranscriptAssistantPart,
) -> bool {
    match (previous, current) {
        (Some(TranscriptAssistantPart::Reasoning(_)), TranscriptAssistantPart::Body(_))
        | (Some(TranscriptAssistantPart::ToolCall(_)), TranscriptAssistantPart::Reasoning(_))
        | (Some(TranscriptAssistantPart::ToolCall(_)), TranscriptAssistantPart::Body(_))
        | (Some(TranscriptAssistantPart::Body(_)), TranscriptAssistantPart::Body(_)) => true,
        (Some(_), TranscriptAssistantPart::Compaction(_))
        | (Some(TranscriptAssistantPart::Compaction(_)), _) => true,
        _ => false,
    }
}

fn turn_has_tool_parts(turn: &TranscriptTurnSection) -> bool {
    turn.assistant_parts.iter().any(|part| {
        matches!(part, TranscriptAssistantPart::ToolCall(_))
    }) || !turn.tool_calls.is_empty()
}

fn body_is_single_line_plain(text: &str) -> bool {
    !text.contains("```") && !text.contains('\n') && !text.trim().is_empty()
}

fn pack_wall_clock_on_line(
    line: &mut Line<'static>,
    clock: &str,
    content_width: u16,
    theme: &Theme,
) {
    let used = line
        .spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum::<usize>();
    let clock_width = display_width(clock);
    let target = usize::from(content_width);
    if clock_width == 0 || used.saturating_add(clock_width) > target {
        return;
    }
    let pad = target.saturating_sub(used).saturating_sub(clock_width);
    if pad > 0 {
        line.spans.push(Span::raw(" ".repeat(pad)));
    }
    line.spans.push(Span::styled(
        clock.to_string(),
        Style::default().fg(theme.text.secondary),
    ));
}

fn context_tool_group_len(parts: &[TranscriptAssistantPart]) -> usize {
    parts
        .iter()
        .take_while(|part| {
            matches!(
                part,
                TranscriptAssistantPart::ToolCall(tool_call)
                    if context_group_tool_id(&tool_call.header.tool_id)
            )
        })
        .count()
}

fn context_tool_group_complete(parts: &[TranscriptAssistantPart]) -> bool {
    parts.iter().all(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if tool_call.header.status == ToolCallDisplayStatus::Succeeded
        )
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "assistant turn surface assembly needs the footer styling inputs in one place"
)]
fn build_assistant_part_render_surface(
    turn: &TranscriptTurnSection,
    part: &TranscriptAssistantPart,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    prepend_gap: bool,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    let mut lines = Vec::new();
    let (
        kind,
        show_outer_rail,
        rail_color,
        surface,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
    ) = match part {
        TranscriptAssistantPart::Reasoning(thinking) => {
            if prepend_gap {
                lines.push(Line::default());
            }
            append_reasoning_block(
                &mut lines,
                thinking,
                theme,
                transcript_surface_content_width(width, false),
                turn.header.status,
                turn.header.status == ActivityStatus::Streaming
                    && turn.body_blocks.is_empty()
                    && turn.tool_calls.is_empty(),
                turn.animation_phase,
                base_surface,
                // Reasoning-only span for Thought for (Grok: 0.1s Thought vs 0.9s Waiting;
                // COMPLETE with no reasoning mono packs Thought for 0.0s, not turn duration).
                turn.header.thinking_duration_ms,
                turn.header.is_selected,
                // Waiting on answers / pending edit permission ⇒ Thought for (Grok freezes).
                waiting_on_answers_label(turn).is_some()
                    || pending_permission_tool_waiting(turn).is_some(),
            );
            (
                TranscriptRenderSurfaceKind::AssistantReasoning,
                false,
                theme.border.subtle,
                base_surface,
                None,
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Body(block) => {
            let content_width = transcript_surface_content_width(width, false);
            let TranscriptBodyBlock::RichText(text) = block;
            // Grok DIFF packs wall clock on the DONE line after tools.
            // Grok TOOL (COUNT=N) and complete (HELLO…) keep a separate clock row.
            // Use turn-has-tools because the no-event fallback path orders Body before tools.
            let pack_clock_on_body = turn_has_tool_parts(turn)
                && body_is_single_line_plain(text)
                && text.trim() == "DONE"
                && turn.footer_timestamp.as_ref().is_some_and(|clock| {
                    let text_width = display_width(text.trim());
                    let clock_width = display_width(clock);
                    text_width.saturating_add(clock_width) < usize::from(content_width)
                });
            if prepend_gap {
                if let Some(clock) = turn.footer_timestamp.as_deref() {
                    if pack_clock_on_body {
                        lines.push(Line::default());
                    } else {
                        lines.push(right_aligned_wall_clock_line(clock, content_width, theme));
                    }
                } else {
                    lines.push(Line::default());
                }
            }
            let selection_rows = if !text.contains("```") {
                Some(selection_rows_for_markdownish_text_block(
                    text,
                    theme.text.primary,
                    TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                    theme,
                    content_width,
                ))
            } else {
                None
            };
            let body_start = lines.len();
            append_rich_text_block(
                &mut lines,
                text,
                theme.text.primary,
                TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                theme,
                content_width,
            );
            if pack_clock_on_body {
                if let Some(clock) = turn.footer_timestamp.as_deref() {
                    if let Some(line) = lines.get_mut(body_start) {
                        pack_wall_clock_on_line(line, clock, content_width, theme);
                    }
                }
            }
            (
                TranscriptRenderSurfaceKind::AssistantBody,
                false,
                assistant_primary_rail_color(turn.header.status, &turn.header.profile_label, theme),
                base_surface,
                None,
                selection_rows,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::ToolCall(tool_call) => {
            let kind = if shell_tool_uses_harness_bash_card(tool_call) {
                TranscriptRenderSurfaceKind::AssistantCommandTool
            } else {
                TranscriptRenderSurfaceKind::AssistantTool
            };
            let render_width = if tool_call_is_todo(tool_call) {
                width
                    .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
                    .max(1)
            } else {
                transcript_surface_render_width(width, kind)
            };
            let render =
                append_tool_call_section_lines(tool_call, theme, render_width, base_surface);
            lines = render.lines;
            (
                kind,
                false,
                transcript_nested_rail_color(theme),
                base_surface,
                Some(render.interaction_rows),
                None,
                render.diff_hunk_offsets,
            )
        }
        TranscriptAssistantPart::Error(error) => {
            append_assistant_error_box(&mut lines, &error.text, theme, width, base_surface);
            (
                TranscriptRenderSurfaceKind::AssistantError,
                false,
                theme.status.error,
                base_surface,
                None,
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Compaction(compaction) => {
            let surface = super::ui_transcript_compaction::build_compaction_render_surface(
                compaction,
                theme,
                width,
                base_surface,
            );
            lines = surface.lines;
            (
                surface.kind,
                surface.show_outer_rail,
                surface.rail_color,
                surface.surface,
                surface.interaction_rows,
                surface.selection_rows,
                surface.diff_hunk_offsets,
            )
        }
    };

    let mut interaction_rows = interaction_rows;
    let mut selection_rows = selection_rows;

    if let Some(rows) = interaction_rows.as_mut() {
        if prepend_gap {
            rows.insert(0, None);
        }
    }

    if let Some(rows) = selection_rows.as_mut() {
        if prepend_gap {
            rows.insert(0, blank_selection_row(width));
        }
    }

    if append_footer {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
            lines.push(Line::default());
            if let Some(rows) = interaction_rows.as_mut() {
                rows.push(None);
            }
            if let Some(rows) = selection_rows.as_mut() {
                rows.push(blank_selection_row(width));
            }
        }
        let footer_line = build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
            transcript_surface_content_width(width, false),
        );
        if let Some(rows) = selection_rows.as_mut() {
            rows.extend(selection_rows_for_rendered_line(&footer_line, width));
        }
        if let Some(rows) = interaction_rows.as_mut() {
            rows.push(None);
        }
        lines.push(footer_line);
    }

    TranscriptRenderSurface {
        kind,
        show_outer_rail,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color,
        surface,
        lines,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
        selected_rail: false,
    }
}

fn build_footer_only_render_surface(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    base_surface: Color,
    width: u16,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantFooter,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: assistant_primary_rail_color(
            turn.header.status,
            &turn.header.profile_label,
            theme,
        ),
        surface: base_surface,
        lines: vec![build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
            transcript_surface_content_width(width, false),
        )],
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
    }
}

pub(super) fn append_reasoning_block(
    lines: &mut Vec<Line<'static>>,
    thinking: &TranscriptLabeledTextSection,
    theme: &Theme,
    width: u16,
    status: ActivityStatus,
    spinner_active: bool,
    animation_phase: usize,
    surface: Color,
    duration_ms: Option<u64>,
    selected: bool,
    force_completed: bool,
) {
    let header_color = thinking_header_color(theme, surface);
    let header_style = Style::default().fg(header_color);

    let (title, body) = reasoning_summary(&thinking.text);
    // Grok question freeze shows Thought for while Waiting on answers (thinking phase done).
    let completed = force_completed
        || !matches!(status, ActivityStatus::Streaming | ActivityStatus::Queued);
    if title.is_none() && body.trim().is_empty() && !completed {
        return;
    }

    let marker = theme.live_shell.transcript_glyphs.tool_marker;
    let header_core = if completed {
        format!(
            "{marker} Thought for {}",
            format_thought_duration_ms(duration_ms.unwrap_or(0))
        )
    } else if spinner_active {
        let spinner = transcript_streaming_spinner_frame(animation_phase);
        match title {
            Some(title) => format!("{spinner} Thinking · {title}"),
            None => format!("{spinner} Thinking"),
        }
    } else {
        match title {
            Some(title) => format!("{marker} Thinking · {title}"),
            None => format!("{marker} Thinking"),
        }
    };
    let _ = selected;
    append_prefixed_wrapped_spans_line(
        lines,
        TRANSCRIPT_REASONING_HEADER_PREFIX,
        header_style,
        vec![Span::styled(header_core, header_style)],
        width,
    );

    let body = reference_reasoning_body_text(&body);
    if body.trim().is_empty() {
        return;
    }

    lines.push(Line::default());
    super::ui_reasoning_markdown::append_reasoning_body_lines(lines, &body, theme, surface, width);
}

fn reference_reasoning_body_text(raw: &str) -> String {
    let clean = raw.replace("[REDACTED]", "");
    if clean.is_empty() {
        return clean;
    }

    let lead_len = clean.bytes().take_while(|byte| *byte == b'\n').count();
    let (lead, body) = clean.split_at(lead_len);
    if let Some(rest) = body.strip_prefix(THINKING_TRACE_LABEL) {
        return format!("{lead}_Thinking:_ {}", rest.trim_start());
    }

    clean
}

fn reasoning_summary(text: &str) -> (Option<String>, String) {
    let content = text.replace("[REDACTED]", "").trim().to_string();
    let Some(after_open) = content.strip_prefix("**") else {
        return (None, content);
    };
    let Some(close_pos) = after_open.find("**") else {
        return (None, content);
    };
    let title = &after_open[..close_pos];
    if title.is_empty() || title.contains('*') || title.contains('\n') || title.contains('\r') {
        return (None, content);
    }

    let after_close = &after_open[close_pos + 2..];
    if after_close.is_empty() {
        return (Some(title.trim().to_string()), String::new());
    }

    let body = if let Some(body) = after_close.strip_prefix("\n\n") {
        body.trim_end().to_string()
    } else if let Some(body) = after_close.strip_prefix("\r\n\r\n") {
        body.trim_end().to_string()
    } else {
        return (None, content);
    };

    (Some(title.trim().to_string()), body)
}

#[expect(
    clippy::too_many_arguments,
    reason = "context tool grouping needs the same surface/footer inputs as other transcript builders"
)]
fn build_context_tool_group_render_surface(
    turn: &TranscriptTurnSection,
    tool_calls: &[&TranscriptToolCallSection],
    theme: &Theme,
    width: u16,
    base_surface: Color,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    let surface = base_surface;
    let mut lines = Vec::new();
    let group_expanded = tool_calls.iter().any(|tool_call| tool_call.expanded);
    let group_disclosure = if tool_calls.is_empty() {
        None
    } else if group_expanded {
        Some(TranscriptToolCallDisclosureState::Expanded)
    } else {
        Some(TranscriptToolCallDisclosureState::Collapsed)
    };
    let (reads, searches, lists, busy) = tool_calls.iter().fold(
        (0usize, 0usize, 0usize, false),
        |(reads, searches, lists, busy), tool_call| {
            let (reads, searches, lists) = match tool_call.header.tool_id.as_str() {
                "fs.read" | "read" => (reads + 1, searches, lists),
                "fs.glob" | "glob" | "fs.grep" | "grep" => (reads, searches + 1, lists),
                "fs.ls" | "list" => (reads, searches, lists + 1),
                _ => (reads, searches, lists),
            };
            (
                reads,
                searches,
                lists,
                busy || !matches!(tool_call.header.status, ToolCallDisplayStatus::Succeeded),
            )
        },
    );

    let mut summary = vec![Span::styled(
        if busy {
            "Gathering context".to_string()
        } else {
            "Gathered context".to_string()
        },
        Style::default().fg(theme.text.primary),
    )];
    let mut counts = Vec::new();
    if reads > 0 {
        counts.push(format!("{reads} read{}", if reads == 1 { "" } else { "s" }));
    }
    if searches > 0 {
        counts.push(format!(
            "{searches} search{}",
            if searches == 1 { "" } else { "es" }
        ));
    }
    if lists > 0 {
        counts.push(format!("{lists} list{}", if lists == 1 { "" } else { "s" }));
    }
    if !counts.is_empty() {
        summary.push(Span::styled(" · ", muted_meta_style(theme)));
        summary.push(Span::styled(counts.join(" · "), muted_meta_style(theme)));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(group_disclosure) {
        summary.push(Span::styled("  ", muted_meta_style(theme)));
        summary.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    append_surface_row(
        &mut lines,
        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
        surface,
        summary,
        transcript_surface_content_width(width, false),
    );

    if group_expanded {
        let detail_prefix = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ");
        for tool_call in tool_calls {
            let mut spans = vec![Span::styled(
                tool_call.header.title.clone(),
                Style::default().fg(theme.text.primary),
            )];
            if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
                spans.push(Span::styled(" · ", muted_meta_style(theme)));
                spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
            }
            append_surface_row(
                &mut lines,
                &detail_prefix,
                surface,
                spans,
                transcript_surface_content_width(width, false),
            );
        }
    }

    if append_footer {
        if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
            lines.push(Line::default());
        }
        lines.push(build_assistant_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            assistant_status,
            theme,
            transcript_surface_content_width(width, false),
        ));
    }

    let mut interaction_rows = vec![None; lines.len()];
    if !interaction_rows.is_empty() {
        interaction_rows[0] = Some(full_width_interaction_row(
            TranscriptMouseTarget::ToolGroup {
                tool_call_ids: tool_calls
                    .iter()
                    .map(|tool_call| tool_call.tool_call_id.clone())
                    .collect(),
            },
        ));
    }

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::AssistantTool,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: transcript_nested_rail_color(theme),
        surface,
        lines,
        interaction_rows: Some(interaction_rows),
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
    }
}

fn build_assistant_footer_line(
    turn: &TranscriptTurnSection,
    assistant_icon: &str,
    assistant_color: Color,
    _assistant_status: &str,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let mut spans = vec![Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string())];

    if let Some(waiting) = waiting_on_answers_label(turn) {
        return pack_waiting_on_answers_footer_line(turn, &waiting, theme, content_width);
    }
    // Grok PERM freeze: ◆ Run Write `path` … right meta while Allow Edit dock is open.
    if let Some(waiting) = pending_permission_tool_waiting(turn) {
        return pack_waiting_on_answers_footer_line(turn, &waiting, theme, content_width);
    }

    if !assistant_icon.is_empty() {
        spans.push(Span::styled(
            format!("{assistant_icon} "),
            Style::default().fg(assistant_color),
        ));
    }

    if matches!(turn.header.status, ActivityStatus::Done | ActivityStatus::Error) {
        if let Some(duration_ms) = turn.header.duration_ms {
            spans.push(Span::styled(
                format!("Worked for {}.", format_thought_duration_ms(duration_ms)),
                muted_meta_style(theme),
            ));
            return Line::from(spans);
        }
    }

    if has_trimmed_content(&turn.header.model_id) {
        spans.push(Span::styled(
            turn.header.model_id.clone(),
            muted_meta_style(theme),
        ));
    } else {
        spans.push(Span::styled(
            assistant_footer_label(&turn.header.profile_label),
            Style::default().fg(assistant_primary_label_color(turn.header.status, theme)),
        ));
    }
    Line::from(spans)
}

fn pack_waiting_on_answers_footer_line(
    turn: &TranscriptTurnSection,
    waiting: &str,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let marker = theme.live_shell.transcript_glyphs.tool_marker;
    let left = format!("{marker} {waiting}");
    let right = waiting_status_right_meta(turn);
    let target = usize::from(content_width);
    let left_width = display_width(&left);
    let right_width = display_width(&right);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if right.is_empty() { 0 } else { 1 });

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(left, muted_meta_style(theme)),
    ];
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    if !right.is_empty() {
        spans.push(Span::styled(right, muted_meta_style(theme)));
    }
    Line::from(spans)
}

fn waiting_status_right_meta(turn: &TranscriptTurnSection) -> String {
    let mut parts = Vec::new();
    if let Some(duration_ms) = turn.header.duration_ms {
        parts.push(format_thought_duration_ms(duration_ms));
    }
    if let Some(total_tokens) = turn.header.total_tokens.filter(|tokens| *tokens > 0) {
        parts.push(format!("⇣{}", format_waiting_token_count(total_tokens)));
    }
    parts.push("[stop]".to_string());
    parts.join(" ")
}

fn format_waiting_token_count(count: u32) -> String {
    if count < 1000 {
        return count.to_string();
    }
    if count < 1_000_000 {
        let thousands = f64::from(count) / 1000.0;
        if count.is_multiple_of(1000) {
            return format!("{}k", count / 1000);
        }
        return format!("{thousands:.1}k");
    }
    format!("{:.1}M", f64::from(count) / 1_000_000.0)
}

/// Pending write/edit tool under permission — Grok PERM freeze packs completed Thought for.
fn pending_permission_tool_waiting(turn: &TranscriptTurnSection) -> Option<String> {
    for part in &turn.assistant_parts {
        let TranscriptAssistantPart::ToolCall(tool) = part else {
            continue;
        };
        if is_question_tool_id(&tool.header.tool_id) {
            continue;
        }
        if !matches!(
            tool.header.status,
            crate::app::ToolCallDisplayStatus::PendingPermission
        ) {
            continue;
        }
        // Grok PERM freeze: "Run Write `path`" while Allow Edit dock is open.
        let title = tool.header.title.trim();
        let label = if let Some(path) = title.strip_prefix("Creating ") {
            format!("Write `{path}`")
        } else if title.is_empty() {
            "tool".to_string()
        } else {
            title.to_string()
        };
        return Some(format!("Run {label}"));
    }
    None
}

fn waiting_on_answers_label(turn: &TranscriptTurnSection) -> Option<String> {
    for part in &turn.assistant_parts {
        let TranscriptAssistantPart::ToolCall(tool) = part else {
            continue;
        };
        if !is_question_tool_id(&tool.header.tool_id) {
            continue;
        }
        if !matches!(
            tool.header.status,
            crate::app::ToolCallDisplayStatus::PendingPermission
                | crate::app::ToolCallDisplayStatus::Queued
                | crate::app::ToolCallDisplayStatus::Running
        ) {
            continue;
        }
        let detail = tool
            .header
            .title
            .strip_prefix("Ask ")
            .unwrap_or(tool.header.title.as_str())
            .trim();
        if detail.is_empty() {
            return Some("Waiting on answers".to_string());
        }
        return Some(format!("Waiting on answers for {detail}"));
    }
    None
}

fn is_question_tool_id(tool_id: &str) -> bool {
    tool_id == "user.question" || tool_id == "question"
}

fn format_thought_duration_ms(duration_ms: u64) -> String {
    if duration_ms >= 60_000 {
        format_duration_ms(duration_ms)
    } else if duration_ms == 0 {
        "0.0s".to_string()
    } else if duration_ms.is_multiple_of(1_000) {
        format!("{}s", duration_ms / 1_000)
    } else {
        format!(
            "{:.1}s",
            f64::from(u32::try_from(duration_ms).unwrap_or(u32::MAX)) / 1_000.0
        )
    }
}

#[cfg(test)]
mod tests {
    use super::reasoning_summary;

    #[test]
    fn reasoning_summary_extracts_title_and_body() {
        let (title, body) = reasoning_summary(
            "**Continuing Quality Review**\n\nDetails.\n\n**Next section**\n\nMore.",
        );
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert_eq!(body, "Details.\n\n**Next section**\n\nMore.");
    }

    #[test]
    fn reasoning_summary_extracts_title_without_body() {
        let (title, body) = reasoning_summary("**Continuing Quality Review**");
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert!(body.is_empty());
    }

    #[test]
    fn reasoning_summary_preserves_indented_body() {
        let (title, body) =
            reasoning_summary("**Continuing Quality Review**\n\n    const value = true\n");
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert_eq!(body, "    const value = true");
    }

    #[test]
    fn reasoning_summary_rejects_inline_bold_title() {
        let (title, body) = reasoning_summary("**Important:** keep this in the body.");
        assert!(title.is_none());
        assert_eq!(body, "**Important:** keep this in the body.");
    }

    #[test]
    fn reasoning_summary_passes_through_plain_text() {
        let (title, body) = reasoning_summary("Details only.");
        assert!(title.is_none());
        assert_eq!(body, "Details only.");
    }

    #[test]
    fn reasoning_summary_strips_redacted_placeholder() {
        let (title, body) = reasoning_summary("[REDACTED]");
        assert!(title.is_none());
        assert!(body.is_empty());
    }

    #[test]
    fn reasoning_summary_strips_redacted_and_extracts_title() {
        let (title, body) = reasoning_summary("[REDACTED]**Title**\n\nbody");
        assert_eq!(title.as_deref(), Some("Title"));
        assert_eq!(body, "body");
    }
}
