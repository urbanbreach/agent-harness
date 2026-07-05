use super::ui_transcript_tool_render::{
    append_assistant_error_box, append_tool_call_section_lines, shell_tool_uses_harness_bash_card,
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
    let surface = transcript_emphasized_surface(theme, base_surface);
    let render_width = transcript_surface_render_width(width, TranscriptRenderSurfaceKind::User);
    let content_width = transcript_surface_content_width(render_width, true);
    let mut lines = vec![user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        Style::default().fg(theme.text.primary),
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
        let agent_accent = theme.agent_accent(&turn.header.profile_label);
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
        Style::default().fg(theme.text.primary),
        surface,
    ));

    TranscriptRenderSurface {
        kind: TranscriptRenderSurfaceKind::User,
        show_outer_rail: true,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: theme.agent_accent(&turn.header.profile_label),
        surface,
        lines,
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
    }
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
        ActivityStatus::Done => ("▪", agent_accent, "done"),
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
            assistant_icon,
            assistant_color,
            assistant_status,
        ));
    }
    surfaces
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
    matches!(
        (previous, current),
        (
            Some(TranscriptAssistantPart::Reasoning(_)),
            TranscriptAssistantPart::Body(_)
        ) | (
            Some(TranscriptAssistantPart::ToolCall(_)),
            TranscriptAssistantPart::Reasoning(_)
        )
    )
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
                transcript_surface_content_width(width, true),
            );
            (
                TranscriptRenderSurfaceKind::AssistantReasoning,
                true,
                theme.border.subtle,
                base_surface,
                None,
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Body(block) => {
            if prepend_gap {
                lines.push(Line::default());
            }
            let selection_rows = match block {
                TranscriptBodyBlock::RichText(text) if !text.contains("```") => {
                    Some(selection_rows_for_markdownish_text_block(
                        text,
                        theme.text.primary,
                        TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                        theme,
                        transcript_surface_content_width(width, false),
                    ))
                }
                _ => None,
            };
            let TranscriptBodyBlock::RichText(text) = block;
            append_rich_text_block(
                &mut lines,
                text,
                theme.text.primary,
                TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                theme,
                transcript_surface_content_width(width, false),
            );
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
            let render_width = transcript_surface_render_width(width, kind);
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
    }
}

fn build_footer_only_render_surface(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    base_surface: Color,
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
        )],
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
    }
}

pub(super) fn append_reasoning_block(
    lines: &mut Vec<Line<'static>>,
    thinking: &TranscriptLabeledTextSection,
    theme: &Theme,
    width: u16,
) {
    let label_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::DIM)
        .add_modifier(Modifier::ITALIC);
    let reasoning_style = Style::default()
        .fg(theme.text.secondary)
        .add_modifier(Modifier::DIM);
    let mut rendered_any_line = false;
    let text = reference_reasoning_body_text(&thinking.text);

    for row in text.lines() {
        let mut spans = Vec::new();
        if !row.is_empty() {
            spans.extend(parse_inline_markdown_spans(
                row,
                reasoning_style,
                theme.text.secondary,
                theme,
            ));
        }
        append_prefixed_wrapped_spans_line(
            lines,
            TRANSCRIPT_REASONING_BODY_PREFIX,
            reasoning_style,
            spans,
            width,
        );
        rendered_any_line = true;
    }

    if !rendered_any_line {
        append_prefixed_wrapped_spans_line(
            lines,
            TRANSCRIPT_REASONING_BODY_PREFIX,
            reasoning_style,
            vec![Span::styled(thinking.label.to_string(), label_style)],
            width,
        );
    }
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
    }
}

fn build_assistant_footer_line(
    turn: &TranscriptTurnSection,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
    theme: &Theme,
) -> Line<'static> {
    let mut spans = vec![Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string())];
    spans.push(Span::styled(
        format!("{} ", assistant_icon),
        Style::default().fg(assistant_color),
    ));
    spans.push(Span::styled(
        assistant_footer_label(&turn.header.profile_label),
        Style::default().fg(assistant_primary_label_color(turn.header.status, theme)),
    ));
    if has_trimmed_content(&turn.header.model_id) {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(
            turn.header.model_id.clone(),
            muted_meta_style(theme),
        ));
    }
    if matches!(
        turn.header.status,
        ActivityStatus::Done | ActivityStatus::Error
    ) {
        if let Some(duration_ms) = turn.header.duration_ms {
            spans.push(Span::styled(" · ", muted_meta_style(theme)));
            spans.push(Span::styled(
                format_duration_ms(duration_ms),
                muted_meta_style(theme),
            ));
        }
    }
    if turn.header.status != ActivityStatus::Done {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(
            assistant_status.to_string(),
            Style::default().fg(assistant_color),
        ));
    }
    if let Some(timestamp) = turn.footer_timestamp.as_deref() {
        spans.push(Span::styled(" · ", muted_meta_style(theme)));
        spans.push(Span::styled(timestamp.to_string(), muted_meta_style(theme)));
    }
    Line::from(spans)
}
