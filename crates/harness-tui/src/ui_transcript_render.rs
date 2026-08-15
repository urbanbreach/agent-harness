// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use super::super::ui_transcript_selection::selection_rows_for_rich_text_block;
use super::ui_streaming_markdown::append_streaming_rich_text_block;
use super::ui_transcript_block_grammar::{
    TranscriptBlockContent, TranscriptBlockRole, TranscriptBlockSpec, TranscriptGrammarError,
    TranscriptPromptState, TranscriptToolDisclosure, TranscriptToolFamily,
    TranscriptToolGroupClass,
};
use super::ui_transcript_style::pending_diamond_color;
use super::ui_transcript_surface::TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH;
use super::ui_transcript_tool_render::{
    append_assistant_error_box, append_tool_call_section_lines, tool_call_is_todo,
};
use super::*;
use crate::app::ToolCallPresentationStatus;
use crate::composer_atoms::split_graphemes;
use harness_core::event::ProviderRequestRetryMetadata;

const USER_MESSAGE_COLLAPSED_MAX_LINES: usize = 3;
const DENSE_GROUP_VISIBLE_MEMBERS: usize = 10;

fn dense_group_start(member_count: usize, command_group: bool, expanded: bool) -> usize {
    if command_group && !expanded {
        member_count.saturating_sub(DENSE_GROUP_VISIBLE_MEMBERS)
    } else {
        0
    }
}

#[expect(
    clippy::manual_unwrap_or_default,
    reason = "invalid grammar atomically omits the section; keep the error policy explicit"
)]
pub(super) fn build_transcript_render_surfaces(
    section: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Vec<ResolvedTranscriptVisualEntryDraft> {
    match try_build_transcript_render_surfaces(section, theme, width, base_surface) {
        Ok(surfaces) => surfaces,
        Err(_) => Vec::new(),
    }
}

pub(in crate::ui) fn try_build_transcript_render_surfaces(
    section: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Result<Vec<ResolvedTranscriptVisualEntryDraft>, TranscriptGrammarError> {
    let specs = super::ui_transcript_block_grammar::normalize_turn_blocks(section);
    try_build_transcript_render_surfaces_with_specs(section, &specs, theme, width, base_surface)
}

pub(in crate::ui) fn try_build_transcript_render_surfaces_with_specs(
    section: &TranscriptTurnSection,
    specs: &[TranscriptBlockSpec],
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Result<Vec<ResolvedTranscriptVisualEntryDraft>, TranscriptGrammarError> {
    let planned_specs = plan_visual_entry_specs(section, specs)?;
    let surfaces = build_turn_render_surfaces(section, theme, width, base_surface)?;
    let mut entries = super::ui_transcript_block_grammar::resolve_entry_surfaces(
        section.activity_first_seq,
        &planned_specs,
        surfaces,
    )?;
    normalize_semantic_entry_chrome(&mut entries, base_surface);
    Ok(entries)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssistantVisualEntryPlan {
    Part { index: usize },
    ToolGroup { start: usize, len: usize },
    Footer,
}

fn assistant_visual_entry_plan(turn: &TranscriptTurnSection) -> Vec<AssistantVisualEntryPlan> {
    let mut plan = Vec::with_capacity(turn.assistant_parts.len().saturating_add(1));
    let mut index = 0;
    while index < turn.assistant_parts.len() {
        let group_len = match &turn.assistant_parts[index] {
            TranscriptAssistantPart::ToolCall(_) => {
                TranscriptToolGroupSummary::from_adjacent(&turn.assistant_parts[index..])
                    .filter(|summary| summary.member_count > 1)
                    .map_or(0, |summary| summary.span_len)
            }
            TranscriptAssistantPart::Reasoning(_)
            | TranscriptAssistantPart::Body(_)
            | TranscriptAssistantPart::Error(_)
            | TranscriptAssistantPart::Compaction(_) => 0,
        };
        if group_len > 1
            && !context_tool_group_uses_individual_rows(
                &turn.assistant_parts[index..index + group_len],
            )
        {
            plan.push(AssistantVisualEntryPlan::ToolGroup {
                start: index,
                len: group_len,
            });
            index += group_len;
        } else {
            plan.push(AssistantVisualEntryPlan::Part { index });
            index += 1;
        }
    }
    if turn.show_footer && !matches!(turn.header.status, ActivityStatus::Error) {
        plan.push(AssistantVisualEntryPlan::Footer);
    }
    plan
}

fn plan_visual_entry_specs(
    turn: &TranscriptTurnSection,
    specs: &[TranscriptBlockSpec],
) -> Result<Vec<TranscriptBlockSpec>, TranscriptGrammarError> {
    let mut planned = Vec::with_capacity(specs.len());
    if turn.user_message.is_some() {
        planned.push(
            specs
                .iter()
                .find(|spec| spec.role == TranscriptBlockRole::UserPrompt)
                .cloned()
                .ok_or(TranscriptGrammarError::InvalidPlacement)?,
        );
    }
    for entry in assistant_visual_entry_plan(turn) {
        let spec = match entry {
            AssistantVisualEntryPlan::Part { index } => {
                let expected =
                    super::ui_transcript_block_grammar::normalized_part_spec(turn, index);
                specs
                    .iter()
                    .find(|spec| {
                        spec.id == expected.id
                            && spec.role == expected.role
                            && spec.content == expected.content
                    })
                    .cloned()
            }
            AssistantVisualEntryPlan::ToolGroup { start, len } => {
                let ids = turn.assistant_parts[start..start + len]
                    .iter()
                    .filter_map(|part| match part {
                        TranscriptAssistantPart::ToolCall(tool) => Some(tool.tool_call_id.as_str()),
                        TranscriptAssistantPart::Reasoning(_)
                        | TranscriptAssistantPart::Body(_)
                        | TranscriptAssistantPart::Error(_)
                        | TranscriptAssistantPart::Compaction(_) => None,
                    })
                    .collect::<Vec<_>>();
                specs
                    .iter()
                    .find(|spec| {
                        matches!(
                            &spec.content,
                            TranscriptBlockContent::Tool {
                                family: TranscriptToolFamily::Group,
                                ids: candidate_ids,
                                ..
                            } if candidate_ids.iter().map(String::as_str).eq(ids.iter().copied())
                        )
                    })
                    .cloned()
            }
            AssistantVisualEntryPlan::Footer => specs
                .iter()
                .find(|spec| spec.role == TranscriptBlockRole::Footer)
                .cloned(),
        }
        .ok_or(TranscriptGrammarError::InvalidGrouping)?;
        planned.push(spec);
    }
    Ok(planned)
}

fn normalize_semantic_entry_chrome(
    entries: &mut [ResolvedTranscriptVisualEntryDraft],
    base_surface: Color,
) {
    let accented_entry = entries
        .iter()
        .rposition(|entry| entry.selected_rail)
        .or_else(|| entries.iter().rposition(|entry| entry.show_outer_rail))
        .or_else(|| {
            entries
                .iter()
                .rposition(|entry| entry.tool_rail_motion.is_some())
        });
    for (index, entry) in entries.iter_mut().enumerate() {
        let previous_surface = entry.surface;
        let preserve_user_surface = entry.kind == TranscriptRenderSurfaceKind::User;
        if previous_surface != base_surface && !preserve_user_surface {
            for span in entry.lines.iter_mut().flat_map(|line| &mut line.spans) {
                if span.style.bg == Some(previous_surface) {
                    span.style.bg = Some(base_surface);
                }
            }
        }
        if !preserve_user_surface {
            entry.surface = base_surface;
        }
        if Some(index) != accented_entry {
            entry.show_outer_rail = false;
            entry.selected_rail = false;
            entry.tool_rail_motion = None;
            entry.metadata.accent = TranscriptVisualEntryAccent::Hidden;
        } else {
            entry.show_outer_rail = true;
        }
    }
}

fn build_turn_render_surfaces(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> Result<Vec<TranscriptVisualEntryDraft>, TranscriptGrammarError> {
    let mut surfaces = Vec::with_capacity(3);
    if turn.user_message.is_some() {
        surfaces.push(build_user_render_surface(turn, theme, width, base_surface)?);
    }
    surfaces.extend(build_assistant_render_surfaces(
        turn,
        theme,
        width,
        base_surface,
    )?);
    Ok(surfaces)
}

fn build_user_render_surface(
    turn: &TranscriptTurnSection,
    theme: &Theme,
    width: u16,
    _base_surface: Color,
) -> Result<TranscriptVisualEntryDraft, TranscriptGrammarError> {
    let spec = super::ui_transcript_block_grammar::normalize_turn_blocks(turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::UserPrompt)
        .ok_or(TranscriptGrammarError::InvalidPlacement)?;
    let TranscriptBlockContent::UserMessage {
        text,
        queued,
        wall_clock,
        state,
    } = &spec.content
    else {
        return Err(TranscriptGrammarError::InvalidPlacement);
    };
    let prompt_state = if turn.header.is_selected {
        TranscriptPromptState::Selected
    } else {
        *state
    };
    let surface = user_prompt_surface(theme, prompt_state);
    let render_width = transcript_surface_render_width(width, TranscriptRenderSurfaceKind::User);
    let content_width = transcript_surface_content_width(render_width, false);
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let body_style = Style::default().fg(theme.text.primary);
    let user_msg = TranscriptUserMessageSection {
        text: text.clone(),
        queued: *queued,
        wall_clock: wall_clock.clone(),
    };
    let lines = build_user_surface_lines(
        &user_msg,
        theme,
        content_width,
        surface,
        agent_accent,
        body_style,
    );

    let raw = TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::User,
        leading_gap_rows: 0,
        placement: TranscriptBlockPlacement::StickyPromptCandidate,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: agent_accent,
        surface,
        lines,
        interaction_rows: None,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    };
    Ok(raw)
}

pub(super) const fn user_prompt_surface(theme: &Theme, state: TranscriptPromptState) -> Color {
    match state {
        TranscriptPromptState::ActiveThinking => theme.reference_terminal.active_prompt_surface,
        TranscriptPromptState::Selected => theme.surface.selected_card,
        TranscriptPromptState::Idle => theme.surface.card,
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
    let body_start = lines.len();
    append_user_surface_text_block_with_first_line_reserve(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_USER_BODY_PREFIX,
        content_width,
        surface,
        first_line_reserve,
    );
    collapse_user_surface_body(&mut lines, body_start, content_width, surface, body_style);
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
        let marker = match theme.glyph_mode() {
            crate::theme::GlyphMode::Preferred => "›",
            crate::theme::GlyphMode::Ascii => ">",
        };
        let marker_prefix = format!("   {marker} ");
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

fn collapse_user_surface_body(
    lines: &mut Vec<Line<'static>>,
    body_start: usize,
    content_width: u16,
    surface: Color,
    body_style: Style,
) {
    let body_rows = lines.len().saturating_sub(body_start);
    if body_rows <= USER_MESSAGE_COLLAPSED_MAX_LINES {
        return;
    }

    lines.truncate(body_start + USER_MESSAGE_COLLAPSED_MAX_LINES);
    let Some(last) = lines.last() else {
        return;
    };
    let prefix = last
        .spans
        .first()
        .map(|span| span.content.to_string())
        .unwrap_or_default();
    let body = last
        .spans
        .iter()
        .skip(1)
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let body_width = usize::from(content_width)
        .saturating_sub(display_width(&prefix))
        .max(1);
    let ellipsis = " …";
    let prefix_text = take_grapheme_width_prefix(
        body.trim_end(),
        body_width.saturating_sub(display_width(ellipsis)),
    );
    let prefix_text = prefix_text.trim_end();
    let collapsed = if prefix_text.is_empty() {
        "…".to_string()
    } else {
        format!("{prefix_text}{ellipsis}")
    };
    if let Some(last) = lines.last_mut() {
        *last = user_surface_line(
            &prefix,
            vec![Span::styled(collapsed, body_style)],
            body_style,
            surface,
        );
    }
}

fn take_grapheme_width_prefix(text: &str, max_width: usize) -> String {
    let mut prefix = String::new();
    let mut used = 0usize;
    for cluster in split_graphemes(text) {
        let width = usize::from(cluster.display_width());
        if used.saturating_add(width) > max_width {
            break;
        }
        prefix.push_str(cluster.as_str());
        used = used.saturating_add(width);
    }
    prefix
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
    let clock_color = if surface == theme.surface.selected_card
        || surface == theme.reference_terminal.active_prompt_surface
    {
        theme.text.primary
    } else {
        theme.text.secondary
    };
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
        Style::default().fg(clock_color),
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
) -> Result<Vec<TranscriptVisualEntryDraft>, TranscriptGrammarError> {
    let agent_accent = theme.agent_accent(&turn.header.profile_label);
    let (assistant_icon, assistant_color, assistant_status) = match turn.header.status {
        ActivityStatus::Queued => (
            theme.live_shell.transcript_glyphs.thought_marker,
            pending_diamond_color(theme, turn.animation_phase),
            "queued",
        ),
        ActivityStatus::Streaming => (
            if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                theme.live_shell.glyphs.streaming
            } else {
                transcript_streaming_spinner_frame_with_motion(
                    turn.animation_phase,
                    turn.motion_enabled,
                )
            },
            agent_accent,
            "active",
        ),
        ActivityStatus::Done => ("", agent_accent, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };

    let mut surfaces = Vec::new();
    for entry in assistant_visual_entry_plan(turn) {
        match entry {
            AssistantVisualEntryPlan::ToolGroup { start, len } => {
                let tool_calls = turn.assistant_parts[start..start + len]
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
                    false,
                    assistant_icon,
                    assistant_color,
                    assistant_status,
                )?);
            }
            AssistantVisualEntryPlan::Part { index } => {
                surfaces.push(build_assistant_part_render_surface(
                    turn,
                    &turn.assistant_parts[index],
                    index,
                    theme,
                    width,
                    base_surface,
                    false,
                    assistant_icon,
                    assistant_color,
                    assistant_status,
                )?)
            }
            AssistantVisualEntryPlan::Footer => surfaces.push(build_footer_only_render_surface(
                turn,
                theme,
                base_surface,
                width,
                assistant_icon,
                assistant_color,
                assistant_status,
            )?),
        }
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
    Ok(surfaces)
}

fn apply_preferred_selected_rail(surfaces: &mut [TranscriptVisualEntryDraft], is_selected: bool) {
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

fn turn_has_tool_parts(turn: &TranscriptTurnSection) -> bool {
    turn.assistant_parts
        .iter()
        .any(|part| matches!(part, TranscriptAssistantPart::ToolCall(_)))
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

fn context_tool_group_uses_individual_rows(parts: &[TranscriptAssistantPart]) -> bool {
    parts.iter().all(|part| {
        matches!(
            part,
            TranscriptAssistantPart::ToolCall(tool_call)
                if matches!(tool_call.header.tool_id.as_str(), "fs.write" | "write" | "edit")
        )
    })
}

fn build_assistant_part_render_surface(
    turn: &TranscriptTurnSection,
    part: &TranscriptAssistantPart,
    part_index: usize,
    theme: &Theme,
    width: u16,
    base_surface: Color,
    append_footer: bool,
    assistant_icon: &str,
    assistant_color: Color,
    assistant_status: &str,
) -> Result<TranscriptVisualEntryDraft, TranscriptGrammarError> {
    let mut lines = Vec::new();
    let mut body_prefix_rows = 0;
    let (
        kind,
        show_outer_rail,
        rail_color,
        surface,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
    ) = match part {
        TranscriptAssistantPart::Reasoning(_) => {
            let content_start = lines.len();
            let spec = super::ui_transcript_block_grammar::normalized_part_spec(turn, part_index);
            let (reasoning_text, reasoning_active, expanded, duration_ms, motion_enabled) =
                match &spec.content {
                    TranscriptBlockContent::Reasoning {
                        text,
                        active,
                        expanded,
                        duration_ms,
                        motion_enabled,
                        ..
                    } => (
                        text.as_str(),
                        *active,
                        *expanded,
                        *duration_ms,
                        *motion_enabled,
                    ),
                    TranscriptBlockContent::UserMessage { .. }
                    | TranscriptBlockContent::AssistantBody { .. }
                    | TranscriptBlockContent::Tool { .. }
                    | TranscriptBlockContent::Footer { .. }
                    | TranscriptBlockContent::Error { .. }
                    | TranscriptBlockContent::Compaction { .. } => ("", false, false, None, false),
                    #[cfg(test)]
                    TranscriptBlockContent::Synthetic { .. } => ("", false, false, None, false),
                };
            append_reasoning_block(
                &mut lines,
                reasoning_text,
                theme,
                transcript_surface_content_width(width, false),
                reasoning_active,
                turn.animation_phase,
                base_surface,
                duration_ms,
                expanded,
                motion_enabled,
                !reasoning_active,
            );
            let mut interaction_rows = vec![None; lines.len()];
            if lines.len() > content_start {
                interaction_rows[content_start] = Some(full_width_interaction_row(
                    TranscriptMouseTarget::Reasoning {
                        request_id: turn.request_id.clone(),
                    },
                ));
            }
            (
                TranscriptRenderSurfaceKind::AssistantReasoning,
                reasoning_active,
                theme.border.subtle,
                base_surface,
                Some(interaction_rows),
                None,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::Body(_) => {
            let content_width = transcript_surface_content_width(width, false);
            let spec = super::ui_transcript_block_grammar::normalized_part_spec(turn, part_index);
            let content = resolve_assistant_body_content(&spec, theme, content_width);
            lines = content.lines;
            body_prefix_rows = content.prefix_rows;
            (
                TranscriptRenderSurfaceKind::AssistantBody,
                false,
                assistant_primary_rail_color(turn.header.status, &turn.header.profile_label, theme),
                base_surface,
                None,
                content.selection_rows,
                Vec::new(),
            )
        }
        TranscriptAssistantPart::ToolCall(tool_call) => {
            let spec = super::ui_transcript_block_grammar::normalized_part_spec(turn, part_index);
            let (family, policy, subagent) = match &spec.content {
                TranscriptBlockContent::Tool {
                    family,
                    policy,
                    subagent,
                    ..
                } => (*family, *policy, subagent.as_ref()),
                _ => return Err(TranscriptGrammarError::InvalidGrouping),
            };
            let kind = if family == TranscriptToolFamily::Execute {
                TranscriptRenderSurfaceKind::AssistantCommandTool
            } else {
                TranscriptRenderSurfaceKind::AssistantTool
            };
            let render_width = if tool_call_is_todo(tool_call) {
                width
                    .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
                    .max(1)
            } else {
                transcript_surface_render_width(
                    width.saturating_sub(policy.trailing_gap_cells),
                    kind,
                )
            };
            let mut render =
                append_tool_call_section_lines(tool_call, theme, render_width, base_surface);
            if subagent.is_some_and(|policy| policy.navigation_target().is_none()) {
                render.interaction_rows.iter_mut().for_each(|row| {
                    if row.as_ref().is_some_and(|interaction| {
                        matches!(
                            interaction.target,
                            TranscriptMouseTarget::SubagentSession { .. }
                        )
                    }) {
                        *row = None;
                    }
                });
            }
            lines = render.lines;
            (
                kind,
                false,
                tool_rail_color(tool_call.header.presentation.status, theme),
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
            let content = super::ui_transcript_compaction::resolve_compaction_content(
                compaction,
                theme,
                width,
                base_surface,
            );
            lines = content.lines;
            (
                TranscriptRenderSurfaceKind::Compaction,
                false,
                theme.border.subtle,
                content.surface,
                None,
                None,
                Vec::new(),
            )
        }
    };
    let tool_rail_motion = match part {
        TranscriptAssistantPart::ToolCall(tool_call) => match tool_call.rail_motion {
            ToolRailMotion::Queued | ToolRailMotion::Settled => None,
            motion => Some(motion),
        },
        TranscriptAssistantPart::Reasoning(_)
        | TranscriptAssistantPart::Body(_)
        | TranscriptAssistantPart::Error(_)
        | TranscriptAssistantPart::Compaction(_) => None,
    };

    let mut interaction_rows = interaction_rows;
    let mut selection_rows = selection_rows;

    if let Some(rows) = interaction_rows.as_mut() {
        for _ in 0..body_prefix_rows {
            rows.insert(0, None);
        }
    }

    if let Some(rows) = selection_rows.as_mut() {
        for _ in 0..body_prefix_rows {
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

    let spec = super::ui_transcript_block_grammar::normalized_part_spec(turn, part_index);
    Ok(TranscriptVisualEntryDraft {
        kind,
        leading_gap_rows: spec.spacing.leading_gap_rows,
        placement: spec.placement,
        show_outer_rail,
        rail_glyph: if tool_rail_motion.is_some() {
            theme.live_shell.transcript_glyphs.rail
        } else {
            TRANSCRIPT_RAIL_GLYPH
        },
        rail_color,
        surface,
        lines,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
        selected_rail: false,
        tool_rail_motion,
    })
}

struct AssistantBodyContent {
    lines: Vec<Line<'static>>,
    selection_rows: Option<Vec<TranscriptSelectionRow>>,
    prefix_rows: usize,
}

fn resolve_assistant_body_content(
    spec: &TranscriptBlockSpec,
    theme: &Theme,
    content_width: u16,
) -> AssistantBodyContent {
    let TranscriptBlockContent::AssistantBody {
        text,
        streaming,
        wall_clock,
        has_tools,
    } = &spec.content
    else {
        return AssistantBodyContent {
            lines: Vec::new(),
            selection_rows: None,
            prefix_rows: 0,
        };
    };
    let pack_clock = body_is_single_line_plain(text) && wall_clock.is_some() && !has_tools;
    let mut lines = Vec::new();
    let prefix_rows = if !pack_clock && spec.spacing.leading_gap_rows > 0 && !text.trim().is_empty()
    {
        wall_clock.as_deref().map_or(0, |clock| {
            lines.push(right_aligned_wall_clock_line(clock, content_width, theme));
            1
        })
    } else {
        0
    };
    let selection_rows = if *streaming {
        None
    } else {
        selection_rows_for_rich_text_block(
            text,
            theme.text.primary,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            theme,
            content_width,
            false,
        )
    };
    if let Some(clock) = wall_clock.as_deref().filter(|_| pack_clock) {
        append_plain_body_with_clock(&mut lines, text, clock, content_width, theme);
    } else if *streaming {
        append_streaming_rich_text_block(
            &mut lines,
            text,
            theme.text.primary,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            theme,
            content_width,
        );
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
    AssistantBodyContent {
        lines,
        selection_rows,
        prefix_rows,
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
) -> Result<TranscriptVisualEntryDraft, TranscriptGrammarError> {
    let spec = super::ui_transcript_block_grammar::normalize_turn_blocks(turn)
        .into_iter()
        .find(|spec| spec.role == TranscriptBlockRole::Footer)
        .ok_or(TranscriptGrammarError::InvalidPlacement)?;
    Ok(TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::AssistantFooter,
        leading_gap_rows: spec.spacing.leading_gap_rows,
        placement: spec.placement,
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
        tool_rail_motion: None,
    })
}

pub(super) fn append_reasoning_block(
    lines: &mut Vec<Line<'static>>,
    thinking_text: &str,
    theme: &Theme,
    width: u16,
    spinner_active: bool,
    animation_phase: usize,
    surface: Color,
    duration_ms: Option<u64>,
    expanded: bool,
    motion_enabled: bool,
    force_completed: bool,
) {
    let header_color = thinking_header_color(theme, surface);
    let header_style = Style::default().fg(header_color);
    let label_style = header_style.add_modifier(Modifier::BOLD);

    let (title, body) = reasoning_summary(thinking_text);
    // Reference question state shows Thought for while Waiting on answers (thinking phase done).
    let completed = force_completed || !spinner_active;
    if title.is_none() && body.trim().is_empty() {
        return;
    }

    let active = spinner_active && !completed;
    let marker = if active {
        if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
            theme.live_shell.glyphs.streaming
        } else {
            transcript_streaming_spinner_frame_with_motion(animation_phase, motion_enabled)
        }
    } else {
        theme.live_shell.transcript_glyphs.tool_marker
    };
    let marker_style = Style::default().fg(if active {
        theme.text.accent
    } else {
        header_color
    });
    let disclosure = if expanded {
        theme.live_shell.transcript_glyphs.disclosure_open
    } else {
        theme.live_shell.transcript_glyphs.disclosure_closed
    };
    let header_spans = if completed {
        let mut spans = vec![Span::styled(format!("{marker} Thought"), label_style)];
        if let Some(duration_ms) = duration_ms {
            spans.push(Span::styled(
                format!(" for {}", format_thought_duration_ms(duration_ms)),
                header_style,
            ));
        }
        spans.push(Span::styled(format!(" {disclosure}"), header_style));
        spans
    } else {
        vec![
            Span::styled(format!("{marker} "), marker_style),
            Span::styled("Thinking…", label_style),
            Span::styled(format!(" {disclosure}"), header_style),
        ]
    };
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

    if completed && !expanded {
        return;
    }

    let body = reference_reasoning_body_text(thinking_text);
    if body.trim().is_empty() {
        return;
    }

    let mut body_lines = Vec::new();
    super::ui_reasoning_markdown::append_reasoning_body_lines(
        &mut body_lines,
        &body,
        theme,
        surface,
        "  ",
        width,
    );
    if !completed && !expanded && body_lines.len() > 3 {
        let preview_start = body_lines.len() - 3;
        let mut preview = vec![Line::from(Span::styled(
            "  …",
            Style::default().fg(theme.text.secondary),
        ))];
        preview.extend(body_lines.drain(preview_start..));
        body_lines = preview;
    }
    lines.push(Line::default());
    lines.extend(body_lines);
    lines.push(Line::default());
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
) -> Result<TranscriptVisualEntryDraft, TranscriptGrammarError> {
    let mut lines = Vec::new();
    let ids = tool_calls
        .iter()
        .map(|tool| tool.tool_call_id.as_str())
        .collect::<Vec<_>>();
    let spec = super::ui_transcript_block_grammar::normalize_turn_blocks(turn)
        .into_iter()
        .find(|spec| match &spec.content {
            TranscriptBlockContent::Tool {
                family: TranscriptToolFamily::Group,
                ids: spec_ids,
                ..
            } => spec_ids.iter().map(String::as_str).eq(ids.iter().copied()),
            _ => false,
        })
        .ok_or(TranscriptGrammarError::InvalidGrouping)?;
    let policy = match &spec.content {
        TranscriptBlockContent::Tool { policy, .. } => policy,
        TranscriptBlockContent::UserMessage { .. }
        | TranscriptBlockContent::AssistantBody { .. }
        | TranscriptBlockContent::Reasoning { .. }
        | TranscriptBlockContent::Footer { .. }
        | TranscriptBlockContent::Error { .. }
        | TranscriptBlockContent::Compaction { .. } => {
            return Err(TranscriptGrammarError::InvalidGrouping);
        }
        #[cfg(test)]
        TranscriptBlockContent::Synthetic { .. } => {
            return Err(TranscriptGrammarError::InvalidGrouping);
        }
    };
    let command_group = policy.group_class == Some(TranscriptToolGroupClass::Commands);
    let surface = if command_group {
        theme.reference_terminal.canvas
    } else {
        base_surface
    };
    let tool_rail_motion = tool_calls.iter().rev().find_map(|tool_call| {
        matches!(
            tool_call.header.presentation.status,
            ToolCallPresentationStatus::Running
        )
        .then_some(tool_call.rail_motion)
        .filter(|motion| matches!(motion, ToolRailMotion::Running { .. }))
    });
    let aggregate_status = dominant_tool_group_status(tool_calls);
    let rail_color = if tool_calls.is_empty() {
        theme.border.subtle
    } else {
        tool_rail_color(aggregate_status, theme)
    };
    let group_expanded = policy.disclosure == TranscriptToolDisclosure::Expanded;
    let group_preview_visible = policy.disclosure == TranscriptToolDisclosure::Preview;
    let group_disclosure = if tool_calls.is_empty() {
        None
    } else if group_expanded {
        Some(TranscriptToolCallDisclosureState::Expanded)
    } else {
        Some(TranscriptToolCallDisclosureState::Collapsed)
    };
    let (reads, searches, lists, skills, busy, failed_commands) = tool_calls.iter().fold(
        (0usize, 0usize, 0usize, 0usize, false, 0usize),
        |(reads, searches, lists, skills, busy, failed_commands), tool_call| {
            let (reads, searches, lists, skills) = match tool_call.header.tool_id.as_str() {
                "fs.read" | "read" => (reads + 1, searches, lists, skills),
                "fs.glob" | "glob" | "fs.grep" | "grep" => (reads, searches + 1, lists, skills),
                "fs.ls" | "list" => (reads, searches, lists + 1, skills),
                "skill" | "skill.load" => (reads, searches, lists, skills + 1),
                _ => (reads, searches, lists, skills),
            };
            let failed_commands = failed_commands
                + usize::from(
                    command_group
                        && tool_call.header.presentation.status
                            == ToolCallPresentationStatus::Failed,
                );
            (
                reads,
                searches,
                lists,
                skills,
                busy || !matches!(
                    tool_call.header.presentation.status,
                    ToolCallPresentationStatus::Succeeded
                        | ToolCallPresentationStatus::Failed
                        | ToolCallPresentationStatus::Cancelled
                ),
                failed_commands,
            )
        },
    );

    let mut summary = if command_group {
        vec![
            Span::styled(
                format!("{} ", theme.live_shell.transcript_glyphs.group_marker),
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
                    tool_calls.len(),
                    if tool_calls.len() == 1 { "" } else { "s" }
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
    if !command_group && skills > 0 {
        counts.push(format!(
            "{skills} skill{}",
            if skills == 1 { "" } else { "s" }
        ));
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
    if let Some(disclosure) = tool_header_disclosure_glyph(group_disclosure, theme) {
        summary.push(Span::styled("  ", muted_meta_style(theme)));
        summary.push(Span::styled(disclosure, muted_meta_style(theme)));
    }

    let summary_prefix = TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string();
    append_surface_row(
        &mut lines,
        &summary_prefix,
        surface,
        summary,
        transcript_surface_content_width(width, false),
    );

    if command_group || group_preview_visible || group_expanded {
        let detail_prefix = format!("{TRANSCRIPT_ASSISTANT_BODY_PREFIX}  ");
        let preview_index = context_group_preview_index(tool_calls);
        let dense_start = policy.visible_start;
        if dense_start > 0 {
            append_surface_row(
                &mut lines,
                &detail_prefix,
                surface,
                vec![Span::styled(
                    format!("… {dense_start} earlier operations"),
                    muted_meta_style(theme),
                )],
                transcript_surface_content_width(width, false),
            );
        }
        let visible_tool_calls = if command_group || group_expanded {
            &tool_calls[dense_start..]
        } else {
            &tool_calls[preview_index..tool_calls.len().min(preview_index + 1)]
        };
        for tool_call in visible_tool_calls {
            let mut spans = Vec::new();
            let member_accent = inline_tool_color(tool_call.header.presentation.status, theme);
            if command_group {
                spans.push(Span::styled(
                    format!("{} ", theme.live_shell.transcript_glyphs.tool_marker),
                    Style::default().fg(member_accent),
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
                        prefix.style = prefix.style.fg(member_accent);
                    }
                }
            }
            if command_group && group_expanded && tool_call.details_visible() {
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

    let raw = TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::AssistantTool,
        leading_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        show_outer_rail: false,
        rail_glyph: theme.live_shell.transcript_glyphs.rail,
        rail_color,
        surface,
        lines,
        interaction_rows: Some(interaction_rows),
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion,
    };
    Ok(raw)
}

fn context_group_preview_index(tool_calls: &[&TranscriptToolCallSection]) -> usize {
    for status in [
        ToolCallPresentationStatus::Waiting,
        ToolCallPresentationStatus::Running,
        ToolCallPresentationStatus::Queued,
    ] {
        if let Some(index) = tool_calls
            .iter()
            .position(|tool_call| tool_call.header.presentation.status == status)
        {
            return index;
        }
    }
    0
}

fn dominant_tool_group_status(
    tool_calls: &[&TranscriptToolCallSection],
) -> ToolCallPresentationStatus {
    for status in [
        ToolCallPresentationStatus::Waiting,
        ToolCallPresentationStatus::Running,
        ToolCallPresentationStatus::Queued,
        ToolCallPresentationStatus::Failed,
        ToolCallPresentationStatus::Cancelled,
        ToolCallPresentationStatus::Succeeded,
    ] {
        if tool_calls
            .iter()
            .any(|tool_call| tool_call.header.presentation.status == status)
        {
            return status;
        }
    }
    ToolCallPresentationStatus::Succeeded
}

const fn tool_rail_color(status: ToolCallPresentationStatus, theme: &Theme) -> Color {
    match status {
        ToolCallPresentationStatus::Running => theme.text.accent,
        ToolCallPresentationStatus::Queued => theme.text.secondary,
        ToolCallPresentationStatus::Waiting => theme.status.warning,
        ToolCallPresentationStatus::Succeeded => theme.status.success,
        ToolCallPresentationStatus::Failed => theme.status.error,
        ToolCallPresentationStatus::Cancelled => theme.status.disabled,
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

    if matches!(turn.header.status, ActivityStatus::Streaming) {
        // Keep the reserved footer row while the bottom dock owns active lifecycle text.
        return Line::default();
    }

    if matches!(turn.header.status, ActivityStatus::Done) {
        let message = turn.header.duration_ms.map_or_else(
            || "Turn completed.".to_string(),
            |duration_ms| {
                format!(
                    "Worked for {}",
                    super::ui_live_turn_status::format_elapsed_ms(duration_ms)
                )
            },
        );
        spans.push(Span::styled(message, muted_meta_style(theme)));
        return Line::from(spans);
    }

    if !assistant_icon.is_empty() {
        spans.push(Span::styled(
            format!("{assistant_icon} "),
            Style::default().fg(assistant_color),
        ));
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

fn retry_attempt(turn: &TranscriptTurnSection) -> Option<&ProviderRequestRetryMetadata> {
    turn.header.retry.as_ref().filter(|retry| retry.attempt > 0)
}

fn pack_waiting_on_answers_footer_line(
    turn: &TranscriptTurnSection,
    waiting: &str,
    theme: &Theme,
    content_width: u16,
) -> Line<'static> {
    let marker = theme.live_shell.transcript_glyphs.tool_marker;
    let label = if waiting.starts_with("Run ") {
        match turn.header.duration_ms {
            Some(duration_ms) => {
                format!("{waiting} {}", format_thought_duration_ms(duration_ms))
            }
            None => waiting.to_string(),
        }
    } else {
        waiting.to_string()
    };
    let right = waiting_status_right_meta(turn);
    let target = assistant_footer_available_width(content_width);
    let marker_width = display_width(marker).saturating_add(1);
    let right_width = display_width(&right);
    let minimum_gap = usize::from(!right.is_empty());
    let label_width = target
        .saturating_sub(marker_width)
        .saturating_sub(right_width)
        .saturating_sub(minimum_gap);
    let label = truncate_plain_text(&label, label_width);
    let left_width = marker_width.saturating_add(display_width(&label));
    let gap = target
        .saturating_sub(left_width)
        .saturating_sub(right_width)
        .max(minimum_gap);

    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX.to_string()),
        Span::styled(
            format!("{marker} "),
            Style::default().fg(pending_diamond_color(theme, turn.animation_phase)),
        ),
        Span::styled(label, Style::default().fg(theme.text.secondary)),
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
            tool.header.presentation.status,
            ToolCallPresentationStatus::Waiting
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
            tool.header.presentation.status,
            ToolCallPresentationStatus::Waiting
                | ToolCallPresentationStatus::Queued
                | ToolCallPresentationStatus::Running
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
        app::{ActivityStatus, ActivityUsage, AppState, ToolCallDisplayStatus},
        theme::Theme,
    };
    use harness_core::event::{
        ActorKind, EventActor, EventEnvelopeV1, EventV1, ProviderRequestFinishedEvent,
        ProviderRequestStartedEvent, ProviderStreamDeltaEvent, ToolCallFinishedEvent,
        ToolCallRequestedEvent, ToolCallStartedEvent, ToolCallStatus, SCHEMA_VERSION,
    };
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    fn lifecycle_event(seq: u64, request_id: &str, payload: EventV1) -> EventEnvelopeV1 {
        EventEnvelopeV1 {
            schema_version: SCHEMA_VERSION,
            event_id: format!("evt_ui11_{seq:04}"),
            seq,
            run_id: "run_ui11".into(),
            mono_ms: seq,
            ts: None,
            actor: EventActor::new(ActorKind::System, Some("ui11-test".to_string())),
            correlation_id: Some(request_id.to_string()),
            causation_id: None,
            stream_key: None,
            payload,
        }
    }

    fn render_app_text(app: &AppState, width: u16, height: u16) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| crate::ui::render_app(frame, app))
            .expect("render app");
        terminal
            .backend()
            .buffer()
            .content
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn ui10_diff_turn(highlight_syntax: bool) -> super::super::TranscriptTurnSection {
        let tool = super::super::TranscriptToolCallSection {
            tool_call_id: "edit-ui10".to_string(),
            coalesced_tool_call_ids: vec!["edit-ui10".to_string()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: super::super::TranscriptToolCallHeader {
                tool_id: "edit".to_string(),
                title: "edit src/lib.rs".to_string(),
                subtitle: None,
                path_metadata: None,
                icon: None,
                presentation: crate::app::ToolCallPresentation::from_display_status(
                    ToolCallDisplayStatus::Succeeded,
                ),
                visual_style: super::super::TranscriptToolCallVisualStyle::Block,
                struck_out: false,
                disclosure_state: None,
            },
            detail_blocks: vec![
                super::super::TranscriptToolCallDetailBlock::StructuredDiff {
                    diff_content: concat!(
                        "--- src/lib.rs\n",
                        "+++ src/lib.rs\n",
                        "@@ -1 +1 @@\n",
                        "-let result = compute(alpha + beta + gamma + delta);\n",
                        "+let result = compute(alpha + beta + gamma + epsilon);\n"
                    )
                    .to_string(),
                    fallback_path: Some("src/lib.rs".to_string()),
                    force_stacked: true,
                    plain_numbered: false,
                    highlight_syntax,
                    show_file_header: false,
                },
            ],
            details_collapsed_by_default: false,
            details_preview_visible: false,
            animation_phase: 0,
            expanded: true,
            rail_motion: super::super::ToolRailMotion::Settled,
        };

        super::super::TranscriptTurnSection {
            activity_first_seq: 1,
            request_id: "request-ui10".to_string(),
            user_message: None,
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            motion_enabled: false,
            reasoning_expanded: false,
            header: super::super::TranscriptTurnHeader {
                status: ActivityStatus::Done,
                is_selected: false,
                provider_request_open: false,
                profile_label: "default".to_string(),
                model_id: "model-ui10".to_string(),
                duration_ms: None,
                thinking_duration_ms: None,
                responding_duration_ms: None,
                total_tokens: None,
                retry: None,
                retry_elapsed_ms: None,
            },
            assistant_parts: vec![super::super::TranscriptAssistantPart::ToolCall(Box::new(
                tool,
            ))],
            assistant_part_source_ids: vec![super::super::TranscriptAssistantPartSourceId(1)],
        }
    }

    fn selected_user_turn() -> super::super::TranscriptTurnSection {
        super::super::TranscriptTurnSection {
            activity_first_seq: 1,
            request_id: "request-selected-user".to_string(),
            user_message: Some(super::super::TranscriptUserMessageSection {
                text: "Selected prompt".to_string(),
                queued: false,
                wall_clock: None,
            }),
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            motion_enabled: false,
            reasoning_expanded: false,
            header: super::super::TranscriptTurnHeader {
                status: ActivityStatus::Done,
                is_selected: true,
                provider_request_open: false,
                profile_label: "default".to_string(),
                model_id: "model-selected-user".to_string(),
                duration_ms: None,
                thinking_duration_ms: None,
                responding_duration_ms: None,
                total_tokens: None,
                retry: None,
                retry_elapsed_ms: None,
            },
            assistant_parts: Vec::new(),
            assistant_part_source_ids: Vec::new(),
        }
    }

    #[test]
    fn selected_user_prompt_uses_temporary_semantic_highlight() {
        let theme = Theme::default();
        let surfaces = build_transcript_render_surfaces(
            &selected_user_turn(),
            &theme,
            80,
            theme.surface.shell,
        );

        assert_eq!(surfaces[0].surface, theme.surface.selected_card);
        assert!(surfaces[0]
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.bg == Some(theme.surface.selected_card)));
    }

    #[test]
    fn syntax_style_upgrade_preserves_diff_text_rows_selection_and_anchors() {
        // Given: one expanded diff measured while its tool is still using provisional styles.
        let theme = Theme::default();
        let width = 36;
        let before_turn = ui10_diff_turn(false);
        let measure = |turn: &super::super::TranscriptTurnSection| {
            super::super::measure_transcript_layout(
                std::slice::from_ref(turn),
                &theme,
                width,
                theme.surface.shell,
                |section| section.activity_first_seq,
                |_index, _section| None,
                |section, theme, width, surface| {
                    build_transcript_render_surfaces(section, theme, width, surface)
                },
            )
        };
        let before = measure(&before_turn);
        let anchor_row = before.sections[0].surfaces[0].height / 2;
        let content_anchor = before
            .capture_content_anchor(anchor_row)
            .expect("diff content anchor");
        let selection_cell = super::super::TranscriptSelectionCell {
            row: anchor_row,
            column: 8,
        };
        let selection_anchor = before
            .capture_selection_anchor(selection_cell)
            .expect("diff selection anchor");
        let before_text = before.sections[0]
            .surfaces
            .iter()
            .flat_map(|surface| surface.lines.iter())
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let before_selection = super::super::transcript_selection_rows(&before, usize::from(width));

        // When: the lifecycle promotes the same diff to full syntax styles.
        let mut after_turn = before_turn.clone();
        for part in &mut after_turn.assistant_parts {
            if let super::super::TranscriptAssistantPart::ToolCall(tool) = part {
                super::super::ui_transcript_tool_sections::set_diff_highlight_phase(
                    &mut tool.detail_blocks,
                    true,
                );
            }
        }
        let after = measure(&after_turn);
        let after_text = after.sections[0]
            .surfaces
            .iter()
            .flat_map(|surface| surface.lines.iter())
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let after_selection = super::super::transcript_selection_rows(&after, usize::from(width));

        // Then: style-only promotion leaves every geometry-bearing projection unchanged.
        assert_eq!(
            (
                after_text,
                after.total_height,
                after_selection,
                after.resolve_content_anchor(content_anchor),
                after.resolve_selection_anchor(selection_anchor),
            ),
            (
                before_text,
                before.total_height,
                before_selection,
                Some(anchor_row),
                Some(selection_cell),
            )
        );
    }

    #[test]
    fn active_lifecycle_has_one_status_source_and_stable_composer_geometry() {
        // Given: a live response with usage metadata, which previously duplicated Responding status.
        const WIDTH: u16 = 120;
        const HEIGHT: u16 = 40;
        let area = Rect::new(0, 0, WIDTH, HEIGHT);
        let request_id = "request-ui11";
        let mut app = AppState::new_live(None, false, None);
        app.ingest_event(lifecycle_event(
            1,
            request_id,
            EventV1::ProviderRequestStarted(ProviderRequestStartedEvent {
                request_id: request_id.into(),
                provider_id: "openai".to_string(),
                model_id: "gpt-5.4-mini".to_string(),
                prompt_summary: "stream then use a tool".to_string(),
                request_digest: "digest-ui11".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(lifecycle_event(
            2,
            request_id,
            EventV1::ProviderStreamDelta(ProviderStreamDeltaEvent {
                request_id: request_id.into(),
                delta: "Working on it".to_string(),
            }),
        ));
        app.activities.back_mut().expect("streaming activity").usage = Some(ActivityUsage {
            prompt_tokens: 100,
            completion_tokens: 20,
            total_tokens: 120,
        });
        let streaming_screen = render_app_text(&app, WIDTH, HEIGHT);
        let streaming_transcript = super::super::transcript_test_line_texts(
            super::super::build_transcript_lines_for_width(&app, &Theme::default(), WIDTH),
        )
        .join("\n");
        let streaming_composer = crate::layout::FrameLayoutPlan::for_app(&app, area)
            .composer
            .expect("streaming composer");

        // When: the same turn moves through a running tool and then terminal completion.
        app.ingest_event(lifecycle_event(
            3,
            request_id,
            EventV1::ToolCallRequested(ToolCallRequestedEvent {
                tool_call_id: "tool-ui11".into(),
                tool_id: "fs.read".to_string(),
                args_summary: r#"{"filePath":"src/lib.rs"}"#.to_string(),
                args_digest: "digest-tool-ui11".to_string(),
                metadata: None,
            }),
        ));
        app.ingest_event(lifecycle_event(
            4,
            request_id,
            EventV1::ToolCallStarted(ToolCallStartedEvent {
                tool_call_id: "tool-ui11".into(),
            }),
        ));
        let tool_screen = render_app_text(&app, WIDTH, HEIGHT);
        let tool_composer = crate::layout::FrameLayoutPlan::for_app(&app, area)
            .composer
            .expect("tool composer");
        app.ingest_event(lifecycle_event(
            5,
            request_id,
            EventV1::ToolCallFinished(ToolCallFinishedEvent {
                tool_call_id: "tool-ui11".into(),
                status: ToolCallStatus::Succeeded,
                output_summary: Some("read src/lib.rs".to_string()),
                output_digest: None,
                output_json: None,
                metadata: None,
            }),
        ));
        app.ingest_event(lifecycle_event(
            6,
            request_id,
            EventV1::ProviderRequestFinished(ProviderRequestFinishedEvent {
                request_id: request_id.into(),
                finish_reason: "stop".to_string(),
                output_digest: None,
                usage: None,
                metadata: None,
            }),
        ));
        let terminal_screen = render_app_text(&app, WIDTH, HEIGHT);
        let terminal_composer = crate::layout::FrameLayoutPlan::for_app(&app, area)
            .composer
            .expect("terminal composer");

        // Then: only the bottom-dock status owns active lifecycle text and the composer never moves.
        assert_eq!(
            (
                streaming_screen.matches("Responding…").count(),
                streaming_transcript.matches("Responding…").count(),
                tool_screen.matches("Run fs.read").count(),
                terminal_screen.matches("Responding…").count()
                    + terminal_screen.matches("Run fs.read").count(),
                tool_composer,
                terminal_composer,
            ),
            (1, 0, 1, 0, streaming_composer, streaming_composer)
        );
    }

    #[test]
    fn ordinary_assistant_text_surfaces_use_themed_base_surface() {
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
                ratatui::style::Color::Rgb(1, 2, 3),
                "ordinary assistant prose must use the themed base surface"
            );
        }
    }

    #[test]
    fn assistant_footer_only_surface_uses_themed_base_surface() {
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
        )
        .expect("valid footer spec");

        assert_eq!(footer.surface, ratatui::style::Color::Rgb(1, 2, 3));
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
    fn command_group_failure_color_stays_on_the_failed_member() {
        // Given: adjacent commands with one successful and one failed member.
        let command = |id: &str, title: &str, status: ToolCallDisplayStatus| {
            super::super::TranscriptToolCallSection {
                tool_call_id: id.to_string(),
                coalesced_tool_call_ids: vec![id.to_string()],
                child_session_id: None,
                subagent_background: false,
                output_truncated: false,
                replay_read_only: false,
                hovered_target: None,
                header: super::super::TranscriptToolCallHeader {
                    tool_id: "shell.run".to_string(),
                    title: title.to_string(),
                    subtitle: None,
                    path_metadata: None,
                    icon: None,
                    presentation: crate::app::ToolCallPresentation::from_display_status(status),
                    visual_style: super::super::TranscriptToolCallVisualStyle::Block,
                    struck_out: false,
                    disclosure_state: None,
                },
                detail_blocks: Vec::new(),
                details_collapsed_by_default: false,
                details_preview_visible: false,
                animation_phase: 0,
                expanded: false,
                rail_motion: super::super::ToolRailMotion::Settled,
            }
        };
        let succeeded = command("command-ok", "echo ok", ToolCallDisplayStatus::Succeeded);
        let failed = command("command-failed", "echo fail", ToolCallDisplayStatus::Failed);
        let turn = super::super::TranscriptTurnSection {
            activity_first_seq: 0,
            request_id: "request-command-colors".to_string(),
            user_message: None,
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            motion_enabled: false,
            reasoning_expanded: false,
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
            assistant_parts: vec![
                super::super::TranscriptAssistantPart::ToolCall(Box::new(succeeded)),
                super::super::TranscriptAssistantPart::ToolCall(Box::new(failed)),
            ],
            assistant_part_source_ids: vec![
                super::super::TranscriptAssistantPartSourceId(1),
                super::super::TranscriptAssistantPartSourceId(2),
            ],
        };

        // When: the mixed-status group is rendered.
        let theme = Theme::default();
        let tools = turn.assistant_tools().collect::<Vec<_>>();
        let group = super::build_context_tool_group_render_surface(
            &turn,
            &tools,
            &theme,
            120,
            ratatui::style::Color::Reset,
            false,
            "",
            ratatui::style::Color::Reset,
            "",
        )
        .expect("valid tool group spec");
        let row_has_error_color = |needle: &str| {
            let row = group.lines.iter().find(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains(needle)
            });
            assert!(row.is_some(), "missing {needle}: {:#?}", group.lines);
            row.expect("command member row")
                .spans
                .iter()
                .any(|span| span.style.fg == Some(theme.reference_terminal.error))
        };

        // Then: aggregate failure chrome does not recolor successful siblings.
        assert!(!row_has_error_color("echo ok"));
        assert!(row_has_error_color("echo fail"));
    }

    #[test]
    fn assistant_tool_surfaces_use_themed_base_surface() {
        fn tool_section(
            id: &str,
            tool_id: &str,
            details: Vec<super::super::TranscriptToolCallDetailBlock>,
        ) -> super::super::TranscriptToolCallSection {
            super::super::TranscriptToolCallSection {
                tool_call_id: id.to_string(),
                coalesced_tool_call_ids: vec![id.to_string()],
                child_session_id: None,
                subagent_background: false,
                output_truncated: false,
                replay_read_only: false,
                hovered_target: None,
                header: super::super::TranscriptToolCallHeader {
                    tool_id: tool_id.to_string(),
                    title: tool_id.to_string(),
                    subtitle: None,
                    path_metadata: None,
                    icon: None,
                    presentation: crate::app::ToolCallPresentation::from_display_status(
                        crate::app::ToolCallDisplayStatus::Succeeded,
                    ),
                    visual_style: super::super::TranscriptToolCallVisualStyle::Block,
                    struck_out: false,
                    disclosure_state: None,
                },
                detail_blocks: details,
                details_collapsed_by_default: false,
                details_preview_visible: false,
                animation_phase: 0,
                expanded: true,
                rail_motion: super::super::ToolRailMotion::Settled,
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
            activity_first_seq: 0,
            request_id: "request-transparent-tools".to_string(),
            user_message: None,
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            motion_enabled: false,
            reasoning_expanded: false,
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
            assistant_parts: vec![
                super::super::TranscriptAssistantPart::ToolCall(Box::new(todo)),
                super::super::TranscriptAssistantPart::Body(
                    super::super::TranscriptBodyBlock::RichText("between tools".to_string()),
                ),
                super::super::TranscriptAssistantPart::ToolCall(Box::new(shell)),
            ],
            assistant_part_source_ids: vec![
                super::super::TranscriptAssistantPartSourceId(1),
                super::super::TranscriptAssistantPartSourceId(2),
                super::super::TranscriptAssistantPartSourceId(3),
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
            assert_eq!(surface.surface, ratatui::style::Color::Rgb(1, 2, 3));
            let backgrounds = surface
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.style.bg)
                .collect::<Vec<_>>();
            assert!(
                backgrounds.iter().all(|background| {
                    background.is_none() || *background == Some(ratatui::style::Color::Rgb(1, 2, 3))
                }),
                "{backgrounds:?}"
            );
        }
    }

    #[test]
    fn settled_provider_failure_uses_error_role_on_themed_base_surface() {
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
            ratatui::style::Color::Rgb(1, 2, 3),
            "flat retry text must use the themed base surface"
        );
        let foregrounds = error
            .lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .filter_map(|span| span.style.fg)
            .collect::<Vec<_>>();
        assert!(!foregrounds.is_empty());
        assert!(
            foregrounds
                .iter()
                .all(|foreground| *foreground == Theme::default().status.error),
            "{foregrounds:?}"
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

    #[test]
    fn dense_command_groups_keep_only_the_latest_ten_members_when_collapsed() {
        assert_eq!(super::dense_group_start(12, true, false), 2);
        assert_eq!(super::dense_group_start(10, true, false), 0);
        assert_eq!(super::dense_group_start(12, true, true), 0);
        assert_eq!(super::dense_group_start(12, false, false), 0);
    }
}
