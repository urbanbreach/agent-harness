// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::ui_transcript_style::pending_diamond_color;
use super::ui_transcript_surface::TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH;
use super::ui_transcript_tool_render::{
    append_assistant_error_box, append_tool_call_section_lines, shell_tool_uses_harness_bash_card,
    tool_call_is_todo,
};
use super::*;
use harness_core::event::ProviderRequestRetryMetadata;

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
    _base_surface: Color,
) -> TranscriptRenderSurface {
    let active_thinking = turn.header.status == ActivityStatus::Streaming
        && turn.thinking.is_some()
        && turn.tool_calls.is_empty();
    let surface = user_prompt_surface(theme, turn.header.is_selected, active_thinking);
    let render_width = transcript_surface_render_width(width, TranscriptRenderSurfaceKind::User);
    let content_width = transcript_surface_content_width(render_width, false);
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let body_style = Style::default().fg(theme.text.primary);
    let lines = build_user_surface_lines(
        user_msg,
        theme,
        content_width,
        surface,
        agent_accent,
        body_style,
    );

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

pub(super) const fn user_prompt_surface(
    theme: &Theme,
    selected: bool,
    active_thinking: bool,
) -> Color {
    if active_thinking {
        theme.reference_terminal.active_prompt_surface
    } else if selected {
        theme.surface.selected_card
    } else {
        theme.surface.card
    }
}

/// Reference scroll state packs max text on the first user row, then right-aligns the
/// wall clock into remaining space. Prefer full-width wrap + pack; only re-wrap
/// with a first-line reserve when the clock cannot fit on the first content row.
fn build_user_surface_lines(
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    content_width: u16,
    surface: Color,
    agent_accent: Color,
    body_style: Style,
) -> Vec<Line<'static>> {
    let clock = user_msg.wall_clock.as_deref();
    let reserve = clock
        .map(|value| display_width(value).saturating_add(1))
        .unwrap_or(0);
    let mut lines = assemble_user_surface_lines(
        user_msg,
        theme,
        content_width,
        surface,
        agent_accent,
        body_style,
        0,
    );
    if let Some(clock) = clock {
        if user_row_has_wall_clock(&lines, clock) {
            return lines;
        }
        lines = assemble_user_surface_lines(
            user_msg,
            theme,
            content_width,
            surface,
            agent_accent,
            body_style,
            reserve,
        );
    }
    lines
}

fn assemble_user_surface_lines(
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    content_width: u16,
    surface: Color,
    agent_accent: Color,
    body_style: Style,
    first_line_reserve: usize,
) -> Vec<Line<'static>> {
    let mut lines = vec![user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        body_style,
        surface,
    )];
    append_user_surface_text_block_with_first_line_reserve(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_USER_BODY_PREFIX,
        content_width,
        surface,
        first_line_reserve,
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
        let marker_prefix = format!("   {} ", theme.live_shell.transcript_glyphs.user_marker);
        lines[1].spans[0] = surface_span(
            marker_prefix,
            Style::default().fg(theme.text.primary),
            surface,
        );
        if let Some(clock) = user_msg.wall_clock.as_deref() {
            append_user_row_wall_clock(&mut lines[1], clock, content_width, theme, surface);
        }
    }
    lines
}

fn user_row_has_wall_clock(lines: &[Line<'static>], clock: &str) -> bool {
    lines
        .get(1)
        .is_some_and(|line| line.spans.iter().any(|span| span.content.as_ref() == clock))
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
        line.spans.push(Span::raw(" ".repeat(pad)));
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
        ActivityStatus::Queued => (
            "◇",
            pending_diamond_color(theme, turn.animation_phase),
            "queued",
        ),
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
            && !context_tool_group_uses_individual_rows(
                &turn.assistant_parts[index..index + group_len],
            )
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
    let paint_selected = turn.header.is_selected
        && matches!(turn.header.status, ActivityStatus::Streaming)
        && turn.header.provider_request_open
        && waiting_on_answers_label(turn).is_none()
        && pending_permission_tool_waiting(turn).is_none()
        && !turn.assistant_parts.iter().any(|part| {
            matches!(
                part,
                TranscriptAssistantPart::ToolCall(tool)
                    if is_question_tool_id(&tool.header.tool_id)
            )
        });
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
            ) && !surface.interaction_rows.as_ref().is_some_and(|rows| {
                rows.iter()
                    .flatten()
                    .any(|row| matches!(&row.target, TranscriptMouseTarget::ToolGroup { .. }))
            })
        })
        .or_else(|| {
            surfaces
                .iter()
                .position(|surface| surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning)
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
        return turn.user_message.is_some().then_some(0);
    }

    // Reference permission/question state: Run Write / Waiting on answers is a separate footer surface.
    // (not glued to the tool body) so layout can pin it just above the dock.
    if pending_permission_tool_waiting(turn).is_some() || waiting_on_answers_label(turn).is_some() {
        return Some(turn.assistant_parts.len());
    }

    // Reference failure/recovery state: auto-retry spinner is a separate footer surface.
    // pinned just above the dock (matching the "Waiting for response…" layout).
    if retry_attempt(turn).is_some() {
        return Some(turn.assistant_parts.len());
    }

    // Reference scroll state: streaming with body text and token usage shows
    // "Responding…" as a separate footer surface pinned just above the dock.
    if matches!(turn.header.status, ActivityStatus::Streaming)
        && !turn.assistant_parts.is_empty()
        && turn.header.total_tokens.is_some_and(|tokens| tokens > 0)
    {
        return Some(turn.assistant_parts.len());
    }

    Some(turn.assistant_parts.len() - 1)
}

fn assistant_part_needs_leading_gap(
    previous: Option<&TranscriptAssistantPart>,
    current: &TranscriptAssistantPart,
) -> bool {
    match (previous, current) {
        (Some(TranscriptAssistantPart::Reasoning(_)), TranscriptAssistantPart::Body(_))
        | (Some(TranscriptAssistantPart::ToolCall(_)), TranscriptAssistantPart::Body(_))
        | (Some(TranscriptAssistantPart::Body(_)), TranscriptAssistantPart::Body(_)) => true,
        (Some(_), TranscriptAssistantPart::Compaction(_))
        | (Some(TranscriptAssistantPart::Compaction(_)), _) => true,
        _ => false,
    }
}

fn turn_has_tool_parts(turn: &TranscriptTurnSection) -> bool {
    turn.assistant_parts
        .iter()
        .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_)))
        || !turn.tool_calls.is_empty()
}

fn body_is_single_line_plain(text: &str) -> bool {
    !text.contains('\n')
        && !text.trim().is_empty()
        && !text
            .chars()
            .any(|ch| matches!(ch, '[' | ']' | '*' | '`' | '<' | '>'))
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

fn append_plain_body_with_clock(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    clock: &str,
    content_width: u16,
    theme: &Theme,
) {
    const TIMESTAMP_RESERVED_WIDTH: usize = 10;

    let text = text.trim();
    let prefix_width = display_width(TRANSCRIPT_ASSISTANT_BODY_PREFIX);
    let text_width = usize::from(content_width)
        .saturating_sub(prefix_width)
        .saturating_sub(TIMESTAMP_RESERVED_WIDTH)
        .max(1);
    let style = Style::default().fg(theme.text.primary);
    for (index, row) in wrap_surface_spans(vec![Span::styled(text.to_string(), style)], text_width)
        .into_iter()
        .enumerate()
    {
        let mut line = Line::from(
            std::iter::once(Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()))
                .chain(row)
                .collect::<Vec<_>>(),
        );
        if index == 0 {
            pack_wall_clock_on_line(&mut line, clock, content_width, theme);
        }
        lines.push(line);
    }
    lines.push(Line::default());
}

fn context_tool_group_len(parts: &[TranscriptAssistantPart]) -> usize {
    let Some(command_group) = parts.first().and_then(transcript_part_tool_group_kind) else {
        return 0;
    };

    parts
        .iter()
        .take_while(|part| {
            matches!(
                part,
                TranscriptAssistantPart::ToolCall(tool_call)
                    if tool_call_group_kind(tool_call) == Some(command_group)
            )
        })
        .count()
}

fn transcript_part_tool_group_kind(tool_call: &TranscriptAssistantPart) -> Option<bool> {
    let TranscriptAssistantPart::ToolCall(tool_call) = tool_call else {
        return None;
    };

    tool_call_group_kind(tool_call)
}

fn tool_call_group_kind(tool_call: &TranscriptToolCallSection) -> Option<bool> {
    match tool_call.header.tool_id.as_str() {
        "shell.run" | "bash" => Some(true),
        tool_id if context_group_tool_id(tool_id) => Some(false),
        _ => None,
    }
}

fn context_tool_group_complete(parts: &[TranscriptAssistantPart]) -> bool {
    parts.iter().all(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if matches!(
                    tool_call.header.status,
                    ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed
                )
        )
    })
}

fn context_tool_group_uses_individual_rows(parts: &[TranscriptAssistantPart]) -> bool {
    parts.iter().all(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if matches!(tool_call.header.tool_id.as_str(), "fs.write" | "write" | "edit")
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
    // Rows prepended before part content (selection/interaction pad must match).
    // Body after Thought with a separate wall clock uses 2 (clock + blank).
    let mut leading_pad_rows = usize::from(prepend_gap);
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
                turn.header.status == ActivityStatus::Streaming,
                turn.animation_phase,
                base_surface,
                // Reasoning-only span for Thought for (0.1s Thought vs 0.9s Waiting).
                turn.header.thinking_duration_ms,
                turn.header.is_selected,
                // Waiting on answers / pending edit permission ⇒ Thought for.
                waiting_on_answers_label(turn).is_some()
                    || pending_permission_tool_waiting(turn).is_some(),
            );
            (
                TranscriptRenderSurfaceKind::AssistantReasoning,
                turn.header.status == ActivityStatus::Streaming,
                theme.border.subtle,
                Color::Reset,
                None,
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Body(block) => {
            let content_width = transcript_surface_content_width(width, false);
            let TranscriptBodyBlock::RichText(text) = block;
            // Reference completed/cancellation state: pack wall clock on a single-line plain body row.
            // Reference diff/tool state: keep wall clock on its own row when tools are present.
            let pack_clock_on_body =
                body_is_single_line_plain(text) && turn.footer_timestamp.is_some();
            // Reference completed state: Thought → blank (inter-surface gap) → body+clock.
            // Reference diff state: Thought → blank → wall-clock row → blank → body.
            if prepend_gap {
                if let Some(clock) = turn.footer_timestamp.as_deref() {
                    if pack_clock_on_body {
                        leading_pad_rows = 0;
                    } else {
                        lines.push(right_aligned_wall_clock_line(clock, content_width, theme));
                        leading_pad_rows = 1;
                    }
                } else {
                    leading_pad_rows = 0;
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
            if let Some(clock) = turn
                .footer_timestamp
                .as_deref()
                .filter(|_| pack_clock_on_body)
            {
                append_plain_body_with_clock(&mut lines, text, clock, content_width, theme);
            } else {
                append_rich_text_block(
                    &mut lines,
                    text,
                    theme.text.primary,
                    TRANSCRIPT_ASSISTANT_BODY_PREFIX,
                    theme,
                    content_width,
                );
            }
            (
                TranscriptRenderSurfaceKind::AssistantBody,
                false,
                assistant_primary_rail_color(turn.header.status, &turn.header.profile_label, theme),
                Color::Reset,
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
                append_tool_call_section_lines(tool_call, theme, render_width, Color::Reset);
            lines = render.lines;
            (
                kind,
                false,
                if tool_call.header.title == "edit" || tool_call.header.title.starts_with("edit ") {
                    theme.reference_terminal.error
                } else {
                    transcript_nested_rail_color(theme)
                },
                Color::Reset,
                Some(render.interaction_rows),
                None,
                render.diff_hunk_offsets,
            )
        }
        TranscriptAssistantPart::Error(error) => {
            append_assistant_error_box(&mut lines, &error.text, theme, width, Color::Reset);
            (
                TranscriptRenderSurfaceKind::AssistantError,
                false,
                theme.status.error,
                Color::Reset,
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
        for _ in 0..leading_pad_rows {
            rows.insert(0, None);
        }
    }

    if let Some(rows) = selection_rows.as_mut() {
        for _ in 0..leading_pad_rows {
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
    _base_surface: Color,
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
        surface: Color::Reset,
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
    let label_style = header_style.add_modifier(Modifier::BOLD);

    let (title, body) = reasoning_summary(&thinking.text);
    // Reference question state shows Thought for while Waiting on answers (thinking phase done).
    let completed =
        force_completed || !matches!(status, ActivityStatus::Streaming | ActivityStatus::Queued);
    if title.is_none() && body.trim().is_empty() && !completed {
        return;
    }

    let marker = theme.live_shell.transcript_glyphs.tool_marker;
    let header_spans = if completed {
        vec![
            Span::styled(format!("{marker} Thought"), label_style),
            Span::styled(
                format!(
                    " for {}",
                    format_thought_duration_ms(duration_ms.unwrap_or(0))
                ),
                header_style,
            ),
        ]
    } else {
        let _ = (spinner_active, animation_phase);
        vec![
            Span::styled(format!("{marker} "), Style::default().fg(header_color)),
            Span::styled("Thinking…", label_style),
        ]
    };
    let _ = selected;
    append_prefixed_wrapped_spans_line(
        lines,
        if completed {
            TRANSCRIPT_REASONING_HEADER_PREFIX
        } else {
            "  "
        },
        header_style,
        header_spans,
        width,
    );

    if completed {
        return;
    }

    let body = reference_reasoning_body_text(&thinking.text);
    if body.trim().is_empty() {
        return;
    }

    lines.push(Line::default());
    super::ui_reasoning_markdown::append_reasoning_body_lines(
        lines, &body, theme, surface, "  ", width,
    );
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
    _base_surface: Color,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> TranscriptRenderSurface {
    let mut lines = Vec::new();
    let command_group = tool_calls
        .iter()
        .all(|tool_call| matches!(tool_call.header.tool_id.as_str(), "shell.run" | "bash"));
    let surface = if command_group {
        theme.reference_terminal.canvas
    } else {
        Color::Reset
    };
    let group_expanded = command_group || tool_calls.iter().any(|tool_call| tool_call.expanded);
    let group_disclosure = if command_group || tool_calls.is_empty() {
        None
    } else if group_expanded {
        Some(TranscriptToolCallDisclosureState::Expanded)
    } else {
        Some(TranscriptToolCallDisclosureState::Collapsed)
    };
    let (reads, searches, lists, busy, successful_commands, failed_commands) =
        tool_calls.iter().fold(
            (0usize, 0usize, 0usize, false, 0usize, 0usize),
            |(reads, searches, lists, busy, successful_commands, failed_commands), tool_call| {
                let (reads, searches, lists) = match tool_call.header.tool_id.as_str() {
                    "fs.read" | "read" => (reads + 1, searches, lists),
                    "fs.glob" | "glob" | "fs.grep" | "grep" => (reads, searches + 1, lists),
                    "fs.ls" | "list" => (reads, searches, lists + 1),
                    _ => (reads, searches, lists),
                };
                let successful_commands = successful_commands
                    + usize::from(
                        command_group
                            && tool_call.header.status == ToolCallDisplayStatus::Succeeded,
                    );
                let failed_commands = failed_commands
                    + usize::from(
                        command_group && tool_call.header.status == ToolCallDisplayStatus::Failed,
                    );
                (
                    reads,
                    searches,
                    lists,
                    busy || !matches!(tool_call.header.status, ToolCallDisplayStatus::Succeeded),
                    successful_commands,
                    failed_commands,
                )
            },
        );

    let mut summary = if command_group {
        vec![
            Span::styled(
                "◈ ",
                Style::default().fg(theme.reference_terminal.secondary),
            ),
            Span::styled(
                "Ran ",
                Style::default()
                    .fg(theme.reference_terminal.secondary)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "{} command{}",
                    successful_commands,
                    if successful_commands == 1 { "" } else { "s" }
                ),
                Style::default().fg(theme.reference_terminal.secondary),
            ),
        ]
    } else {
        vec![Span::styled(
            if busy {
                "Gathering context".to_string()
            } else {
                "Gathered context".to_string()
            },
            Style::default().fg(theme.text.primary),
        )]
    };
    let mut counts = Vec::new();
    if command_group && failed_commands > 0 {
        counts.push(format!("{failed_commands} failed"));
    }
    if !command_group && reads > 0 {
        counts.push(format!("{reads} read{}", if reads == 1 { "" } else { "s" }));
    }
    if !command_group && searches > 0 {
        counts.push(format!(
            "{searches} search{}",
            if searches == 1 { "" } else { "es" }
        ));
    }
    if !command_group && lists > 0 {
        counts.push(format!("{lists} list{}", if lists == 1 { "" } else { "s" }));
    }
    if !counts.is_empty() {
        let count_style = if command_group {
            Style::default().fg(theme.reference_terminal.error)
        } else {
            muted_meta_style(theme)
        };
        summary.push(Span::styled(" · ", count_style));
        summary.push(Span::styled(counts.join(" · "), count_style));
    }
    if let Some(disclosure) = tool_header_disclosure_glyph(group_disclosure) {
        summary.push(Span::styled("  ", muted_meta_style(theme)));
        summary.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    let summary_prefix = if command_group {
        format!("{TRANSCRIPT_SELECTED_RAIL_GLYPH}  ")
    } else {
        TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()
    };
    append_surface_row(
        &mut lines,
        &summary_prefix,
        surface,
        summary,
        transcript_surface_content_width(width, false),
    );

    if group_expanded {
        let detail_prefix = if command_group {
            format!("{TRANSCRIPT_SELECTED_RAIL_GLYPH}  ")
        } else {
            format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ")
        };
        let command_group_accent = if failed_commands > 0 {
            theme.reference_terminal.error
        } else {
            theme.text.primary
        };
        for tool_call in tool_calls {
            let mut spans = Vec::new();
            if command_group {
                spans.push(Span::styled(
                    "◆ ",
                    Style::default().fg(command_group_accent),
                ));
                spans.push(Span::styled(
                    "Run ",
                    Style::default()
                        .fg(theme.reference_terminal.secondary)
                        .add_modifier(Modifier::BOLD),
                ));
                spans.push(Span::styled(
                    command_group_member_command(tool_call),
                    Style::default().fg(theme.reference_terminal.secondary),
                ));
            } else {
                spans.push(Span::styled(
                    tool_call.header.title.clone(),
                    Style::default().fg(theme.text.primary),
                ));
            }
            if let Some(subtitle) = tool_call.header.subtitle.as_deref() {
                spans.push(Span::styled(" · ", muted_meta_style(theme)));
                spans.push(Span::styled(subtitle.to_string(), muted_meta_style(theme)));
            }
            let member_start = lines.len();
            append_surface_row(
                &mut lines,
                &detail_prefix,
                surface,
                spans,
                transcript_surface_content_width(width, false),
            );
            if command_group {
                for line in &mut lines[member_start..] {
                    if let Some(prefix) = line.spans.first_mut() {
                        prefix.style = prefix.style.fg(command_group_accent);
                    }
                }
            }
            if command_group && tool_call.details_visible() {
                let mut detail_render = ToolSectionRender {
                    lines: Vec::new(),
                    interaction_rows: Vec::new(),
                    diff_hunk_offsets: Vec::new(),
                };
                super::ui_transcript_tool_render::append_tool_call_detail_blocks(
                    &mut detail_render,
                    tool_call,
                    theme,
                    width,
                    surface,
                    None,
                );
                lines.extend(detail_render.lines);
            }
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
        rail_color: if command_group {
            theme.surface.card
        } else {
            transcript_nested_rail_color(theme)
        },
        surface,
        lines,
        interaction_rows: Some(interaction_rows),
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
    }
}

fn command_group_member_command(tool_call: &TranscriptToolCallSection) -> String {
    let command = tool_call
        .detail_blocks
        .iter()
        .find_map(|detail| match detail {
            TranscriptToolCallDetailBlock::BashPanel { command, .. } => Some(command.as_str()),
            _ => None,
        });
    let command = command.unwrap_or(tool_call.header.title.as_str());
    command.to_string()
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
    // Reference permission state: ◆ Run Write `path` … right meta while Allow Edit dock is open.
    if let Some(waiting) = pending_permission_tool_waiting(turn) {
        return pack_waiting_on_answers_footer_line(turn, &waiting, theme, content_width);
    }

    // Auto-retry chrome: streaming with retry metadata shows
    // "⠸ Retrying (attempt N)… <retry-elapsed>" with total duration + tokens + [stop] right meta.
    if matches!(turn.header.status, ActivityStatus::Streaming) {
        if let Some(retry) = retry_attempt(turn) {
            return pack_retrying_footer_line(
                turn,
                retry,
                assistant_icon,
                assistant_color,
                theme,
                content_width,
            );
        }
    }

    // Reference streaming state: streaming with no body text but with token usage
    // (ProviderRequestFinished seeded total_tokens) shows "Waiting for response…"
    // with elapsed time + download counter + [stop] right meta.
    if matches!(turn.header.status, ActivityStatus::Streaming)
        && turn.body_blocks.is_empty()
        && (turn.assistant_parts.is_empty() || turn_has_only_terminal_tool_calls(turn))
        && turn.header.total_tokens.is_some_and(|tokens| tokens > 0)
    {
        return pack_waiting_for_response_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            theme,
            content_width,
        );
    }

    // Reference scroll state: streaming with body text and token usage
    // shows "Responding…" with responding-elapsed (time since first delta)
    // + total duration + download counter + [stop] right meta.
    if matches!(turn.header.status, ActivityStatus::Streaming)
        && !turn.assistant_parts.is_empty()
        && turn.header.total_tokens.is_some_and(|tokens| tokens > 0)
    {
        return pack_responding_footer_line(
            turn,
            assistant_icon,
            assistant_color,
            theme,
            content_width,
        );
    }

    if !assistant_icon.is_empty() {
        spans.push(Span::styled(
            format!("{assistant_icon} "),
            Style::default().fg(assistant_color),
        ));
    }

    if matches!(
        turn.header.status,
        ActivityStatus::Done | ActivityStatus::Error
    ) {
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

fn turn_has_only_terminal_tool_calls(turn: &TranscriptTurnSection) -> bool {
    !turn.tool_calls.is_empty()
        && turn.tool_calls.iter().all(|tool_call| {
            matches!(
                tool_call.header.status,
                ToolCallDisplayStatus::Succeeded | ToolCallDisplayStatus::Failed
            )
        })
}

fn pack_waiting_for_response_footer_line(
    turn: &TranscriptTurnSection,
    assistant_icon: &str,
    _assistant_color: Color,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let mut left_text = String::new();
    if !assistant_icon.is_empty() {
        left_text.push_str(assistant_icon);
        left_text.push(' ');
    }
    left_text.push_str("Waiting for response…");
    if let Some(duration_ms) = turn.header.duration_ms {
        left_text.push(' ');
        left_text.push_str(&format_thought_duration_ms(duration_ms));
    }
    let right = waiting_status_right_meta(turn);
    let target = assistant_footer_available_width(content_width);
    let left_width = display_width(&left_text);
    let right_width = display_width(&right);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if right.is_empty() { 0 } else { 1 });

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(left_text, Style::default().fg(theme.text.secondary)),
    ];
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    if !right.is_empty() {
        spans.push(Span::styled(right, muted_meta_style(theme)));
    }
    Line::from(spans)
}

fn pack_responding_footer_line(
    turn: &TranscriptTurnSection,
    assistant_icon: &str,
    _assistant_color: Color,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let mut left_text = String::new();
    if !assistant_icon.is_empty() {
        left_text.push_str(assistant_icon);
        left_text.push(' ');
    }
    left_text.push_str("Responding…");
    if let Some(duration_ms) = turn.header.responding_duration_ms {
        left_text.push(' ');
        left_text.push_str(&format_thought_duration_ms(duration_ms));
    }
    let right = waiting_status_right_meta(turn);
    let target = assistant_footer_available_width(content_width);
    let left_width = display_width(&left_text);
    let right_width = display_width(&right);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if right.is_empty() { 0 } else { 1 });

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(left_text, Style::default().fg(theme.text.secondary)),
    ];
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    if !right.is_empty() {
        spans.push(Span::styled(right, muted_meta_style(theme)));
    }
    Line::from(spans)
}

fn retry_attempt(turn: &TranscriptTurnSection) -> Option<&ProviderRequestRetryMetadata> {
    turn.header.retry.as_ref().filter(|retry| retry.attempt > 0)
}

fn pack_retrying_footer_line(
    turn: &TranscriptTurnSection,
    retry: &ProviderRequestRetryMetadata,
    assistant_icon: &str,
    _assistant_color: Color,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let mut left_text = String::new();
    if !assistant_icon.is_empty() {
        left_text.push_str(assistant_icon);
        left_text.push(' ');
    }
    left_text.push_str(&format!("Retrying (attempt {})…", retry.attempt));
    if let Some(elapsed_ms) = turn.header.retry_elapsed_ms {
        left_text.push(' ');
        left_text.push_str(&format_thought_duration_ms(elapsed_ms));
    }
    let right = waiting_status_right_meta(turn);
    let target = assistant_footer_available_width(content_width);
    let left_width = display_width(&left_text);
    let right_width = display_width(&right);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if right.is_empty() { 0 } else { 1 });

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(left_text, Style::default().fg(theme.status.warning)),
    ];
    if gap > 0 {
        spans.push(Span::raw(" ".repeat(gap)));
    }
    if !right.is_empty() {
        spans.push(Span::styled(right, muted_meta_style(theme)));
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
    let left = if waiting.starts_with("Run ") {
        match turn.header.duration_ms {
            Some(duration_ms) => format!(
                "{marker} {waiting} {}",
                format_thought_duration_ms(duration_ms)
            ),
            None => format!("{marker} {waiting}"),
        }
    } else {
        format!("{marker} {waiting}")
    };
    let right = waiting_status_right_meta(turn);
    let target = assistant_footer_available_width(content_width);
    let left_width = display_width(&left);
    let right_width = display_width(&right);
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(if right.is_empty() { 0 } else { 1 });

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(left, Style::default().fg(theme.text.secondary)),
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

fn assistant_footer_available_width(content_width: u16) -> usize {
    usize::from(content_width).saturating_sub(display_width(TRANSCRIPT_ASSISTANT_BODY_PREFIX))
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
        if count < 10_000 {
            return format!("{thousands:.2}k");
        }
        return format!("{thousands:.1}k");
    }
    format!("{:.1}M", f64::from(count) / 1_000_000.0)
}

/// Pending write/edit tool under permission — reference permission state packs completed Thought for.
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
        // Reference permission state: "Run Write `path`" while Allow Edit dock is open.
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
    use super::{build_transcript_render_surfaces, reasoning_summary};
    use crate::{
        app::{ActivityStatus, AppState},
        theme::Theme,
    };

    #[test]
    fn ordinary_assistant_text_surfaces_do_not_paint_a_full_width_background() {
        let mut app = AppState::default();
        let mut entry = super::super::transcript_section_model_test_activity(
            "request-transparent-assistant-text",
            ActivityStatus::Done,
            "plain assistant answer",
        );
        entry.thinking_text = "plain reasoning".to_string();
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.transcript_view.selected_activity_index = 0;

        let sections = super::super::build_transcript_sections(&app);
        let surfaces = build_transcript_render_surfaces(
            &sections[0],
            &Theme::default(),
            120,
            ratatui::style::Color::Rgb(1, 2, 3),
        );

        for surface in surfaces.into_iter().filter(|surface| {
            matches!(
                surface.kind,
                super::super::TranscriptRenderSurfaceKind::AssistantReasoning
                    | super::super::TranscriptRenderSurfaceKind::AssistantBody
            )
        }) {
            assert_eq!(
                surface.surface,
                ratatui::style::Color::Reset,
                "ordinary assistant prose must inherit the terminal background"
            );
        }
    }

    #[test]
    fn assistant_footer_only_surface_does_not_paint_a_full_width_background() {
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            super::super::transcript_section_model_test_activity(
                "request-transparent-assistant-footer",
                ActivityStatus::Streaming,
                "",
            ),
        ]);
        app.transcript_view.selected_activity_index = 0;

        let sections = super::super::build_transcript_sections(&app);
        let footer = super::build_footer_only_render_surface(
            &sections[0],
            &Theme::default(),
            ratatui::style::Color::Rgb(1, 2, 3),
            120,
            "",
            ratatui::style::Color::Reset,
            "",
        );

        assert_eq!(footer.surface, ratatui::style::Color::Reset);
    }

    #[test]
    fn initial_provider_attempt_does_not_render_retry_chrome() {
        let mut app = AppState::default();
        let mut entry = super::super::transcript_section_model_test_activity(
            "request-initial-provider-attempt",
            ActivityStatus::Streaming,
            "",
        );
        entry.request_data = Some(harness_core::event::ProviderRequestStartedEvent {
            request_id: entry.request_id.clone().into(),
            provider_id: "default".to_string(),
            model_id: entry.model_id.clone(),
            prompt_summary: "initial request".to_string(),
            request_digest: "digest-initial-request".to_string(),
            metadata: Some(harness_core::event::ProviderRequestStartedMetadata {
                retry: Some(harness_core::event::ProviderRequestRetryMetadata {
                    attempt: 0,
                    max_attempts: 3,
                    delay_ms: None,
                    category: None,
                }),
                ..harness_core::event::ProviderRequestStartedMetadata::default()
            }),
        });
        entry.usage = Some(crate::app::ActivityUsage {
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
        });
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.transcript_view.selected_activity_index = 0;

        let sections = super::super::build_transcript_sections(&app);
        let surfaces = build_transcript_render_surfaces(
            &sections[0],
            &Theme::default(),
            120,
            ratatui::style::Color::Reset,
        );
        let text = surfaces
            .iter()
            .flat_map(|surface| surface.lines.iter())
            .flat_map(|line| line.spans.iter())
            .map(|span| span.content.as_ref())
            .collect::<String>();

        assert!(text.is_empty(), "{text}");
    }

    #[test]
    fn assistant_tool_surfaces_do_not_paint_tool_cards() {
        fn tool_section(
            id: &str,
            tool_id: &str,
            details: Vec<super::super::TranscriptToolCallDetailBlock>,
        ) -> super::super::TranscriptToolCallSection {
            super::super::TranscriptToolCallSection {
                tool_call_id: id.to_string(),
                child_session_id: None,
                hovered_target: None,
                header: super::super::TranscriptToolCallHeader {
                    tool_id: tool_id.to_string(),
                    title: tool_id.to_string(),
                    subtitle: None,
                    path_metadata: None,
                    icon: None,
                    status: crate::app::ToolCallDisplayStatus::Succeeded,
                    visual_style: super::super::TranscriptToolCallVisualStyle::Block,
                    struck_out: false,
                    disclosure_state: None,
                },
                detail_blocks: details,
                details_collapsed_by_default: false,
                details_preview_visible: false,
                animation_phase: 0,
                expanded: true,
            }
        }

        let theme = Theme::default();
        let todo = tool_section(
            "todo-card",
            "todo.write",
            vec![super::super::TranscriptToolCallDetailBlock::TodoList {
                items: vec![super::super::TranscriptTodoItem {
                    content: "remove the panel surface".to_string(),
                    status: crate::ui::ui_tool_question_todo::TranscriptTodoStatus::Completed,
                }],
            }],
        );
        let shell = tool_section(
            "shell-card",
            "shell.run",
            vec![super::super::TranscriptToolCallDetailBlock::BashPanel {
                command: "printf tool-card".to_string(),
                output: "tool-card".to_string(),
                description: Some("Shell".to_string()),
                expand_hint: None,
                tone: super::super::TranscriptToolCallDetailTone::Primary,
            }],
        );
        let turn = super::super::TranscriptTurnSection {
            request_id: "request-transparent-tools".to_string(),
            user_message: None,
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            header: super::super::TranscriptTurnHeader {
                status: ActivityStatus::Done,
                is_selected: false,
                provider_request_open: false,
                profile_label: "default".to_string(),
                model_id: "model".to_string(),
                duration_ms: None,
                thinking_duration_ms: None,
                responding_duration_ms: None,
                total_tokens: None,
                retry: None,
                retry_elapsed_ms: None,
            },
            body_blocks: Vec::new(),
            tool_calls: vec![todo.clone(), shell.clone()],
            thinking: None,
            error: None,
            assistant_parts: vec![
                super::super::TranscriptAssistantPart::ToolCall(Box::new(todo)),
                super::super::TranscriptAssistantPart::Body(
                    super::super::TranscriptBodyBlock::RichText("between tools".to_string()),
                ),
                super::super::TranscriptAssistantPart::ToolCall(Box::new(shell)),
            ],
        };

        let surfaces = build_transcript_render_surfaces(
            &turn,
            &theme,
            120,
            ratatui::style::Color::Rgb(1, 2, 3),
        );

        for surface in surfaces.into_iter().filter(|surface| {
            matches!(
                surface.kind,
                super::super::TranscriptRenderSurfaceKind::AssistantTool
                    | super::super::TranscriptRenderSurfaceKind::AssistantCommandTool
            )
        }) {
            assert_eq!(surface.surface, ratatui::style::Color::Reset);
            let backgrounds = surface
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.style.bg)
                .collect::<Vec<_>>();
            assert!(
                backgrounds.iter().all(|background| {
                    background.is_none() || *background == Some(ratatui::style::Color::Reset)
                }),
                "{backgrounds:?}"
            );
        }
    }

    #[test]
    fn flat_assistant_error_surface_does_not_paint_a_full_width_background() {
        let mut app = AppState::default();
        let mut entry = super::super::transcript_section_model_test_activity(
            "request-transparent-assistant-error",
            ActivityStatus::Error,
            "",
        );
        entry.error_message = Some("network unavailable".to_string());
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.transcript_view.selected_activity_index = 0;

        let sections = super::super::build_transcript_sections(&app);
        let surfaces = build_transcript_render_surfaces(
            &sections[0],
            &Theme::default(),
            120,
            ratatui::style::Color::Rgb(1, 2, 3),
        );
        let error = surfaces
            .into_iter()
            .find(|surface| {
                surface.kind == super::super::TranscriptRenderSurfaceKind::AssistantError
            })
            .expect("error activity must have an assistant-error surface");

        assert_eq!(
            error.surface,
            ratatui::style::Color::Reset,
            "flat retry text must inherit the terminal background"
        );
    }

    #[test]
    fn reasoning_summary_extracts_title_and_body() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary(
            "**Continuing Quality Review**\n\nDetails.\n\n**Next section**\n\nMore.",
        );
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert_eq!(body, "Details.\n\n**Next section**\n\nMore.");
    }

    #[test]
    fn reasoning_summary_extracts_title_without_body() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary("**Continuing Quality Review**");
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert!(body.is_empty());
    }

    #[test]
    fn reasoning_summary_preserves_indented_body() {
        // arrange
        // act
        // assert
        let (title, body) =
            reasoning_summary("**Continuing Quality Review**\n\n    const value = true\n");
        assert_eq!(title.as_deref(), Some("Continuing Quality Review"));
        assert_eq!(body, "    const value = true");
    }

    #[test]
    fn reasoning_summary_rejects_inline_bold_title() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary("**Important:** keep this in the body.");
        assert!(title.is_none());
        assert_eq!(body, "**Important:** keep this in the body.");
    }

    #[test]
    fn reasoning_summary_passes_through_plain_text() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary("Details only.");
        assert!(title.is_none());
        assert_eq!(body, "Details only.");
    }

    #[test]
    fn reasoning_summary_strips_redacted_placeholder() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary("[REDACTED]");
        assert!(title.is_none());
        assert!(body.is_empty());
    }

    #[test]
    fn reasoning_summary_strips_redacted_and_extracts_title() {
        // arrange
        // act
        // assert
        let (title, body) = reasoning_summary("[REDACTED]**Title**\n\nbody");
        assert_eq!(title.as_deref(), Some("Title"));
        assert_eq!(body, "body");
    }
}
