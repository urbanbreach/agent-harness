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
use std::time::Duration;

const USER_MESSAGE_COLLAPSED_MAX_LINES: usize = 3;
const USER_TIMESTAMP_RESERVED_WIDTH: u16 = 10;
const USER_TIMESTAMP_RIGHT_PADDING_WIDTH: u16 =
    super::super::ui_transcript_surface::TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH;
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

#[derive(Clone, Debug)]
enum AssistantVisualEntryPlan {
    Part { index: usize },
    ToolGroup(Box<super::ui_transcript_groups::TranscriptToolGroup>),
    Footer,
}

fn assistant_visual_entry_plan(turn: &TranscriptTurnSection) -> Vec<AssistantVisualEntryPlan> {
    let mut plan = Vec::with_capacity(turn.assistant_parts.len().saturating_add(1));
    let groups = super::ui_transcript_groups::scan(turn);
    let mut hidden = vec![false; turn.assistant_parts.len()];
    for group in &groups {
        for &index in &group.hidden {
            hidden[index] = true;
        }
    }
    let mut groups = groups.into_iter().peekable();
    for (index, hidden) in hidden.into_iter().enumerate() {
        if let Some(group) = groups.next_if(|group| group.start == index) {
            plan.push(AssistantVisualEntryPlan::ToolGroup(Box::new(group)));
        }
        if !hidden {
            plan.push(AssistantVisualEntryPlan::Part { index });
        }
    }
    if turn.show_footer
        && !matches!(turn.header.status, ActivityStatus::Error)
        && (turn.header.status != ActivityStatus::Streaming
            || waiting_on_answers_label(turn).is_some()
            || pending_permission_tool_waiting(turn).is_some())
    {
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
            AssistantVisualEntryPlan::ToolGroup(group) => {
                let ids = group
                    .target_ids
                    .iter()
                    .map(String::as_str)
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
    for entry in entries {
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
        expanded_wall_clock: turn
            .user_message
            .as_ref()
            .and_then(|message| message.expanded_wall_clock.clone()),
        wall_clock_hovered: turn
            .user_message
            .as_ref()
            .is_some_and(|message| message.wall_clock_hovered),
    };
    let lines = build_user_surface_lines(
        &user_msg,
        theme,
        content_width,
        surface,
        agent_accent,
        body_style,
    );

    let interaction_rows = user_msg.wall_clock.as_ref().map(|_| {
        let mut rows = vec![None; lines.len()];
        let timestamp_end = content_width.saturating_sub(USER_TIMESTAMP_RIGHT_PADDING_WIDTH);
        if timestamp_end > USER_TIMESTAMP_RESERVED_WIDTH.saturating_add(1) {
            if let Some(row) = rows.get_mut(1) {
                *row = Some(TranscriptInteractionRow {
                    target: TranscriptMouseTarget::UserTimestamp {
                        request_id: turn.request_id.clone(),
                    },
                    hit_start: timestamp_end.saturating_sub(USER_TIMESTAMP_RESERVED_WIDTH),
                    hit_width: USER_TIMESTAMP_RESERVED_WIDTH,
                });
            }
        }
        rows
    });

    let raw = TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::User,
        leading_gap_rows: 0,
        trailing_gap_rows: 0,
        placement: TranscriptBlockPlacement::StickyPromptCandidate,
        show_outer_rail: false,
        rail_glyph: TRANSCRIPT_RAIL_GLYPH,
        rail_color: agent_accent,
        surface,
        lines,
        interaction_rows,
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: None,
    };
    Ok(raw)
}

pub(super) const fn user_prompt_surface(theme: &Theme, state: TranscriptPromptState) -> Color {
    match state {
        TranscriptPromptState::Selected => theme.surface.selected_card,
        TranscriptPromptState::ActiveThinking | TranscriptPromptState::Idle => theme.surface.card,
    }
}

fn build_user_surface_lines(
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    content_width: u16,
    surface: Color,
    agent_accent: Color,
    body_style: Style,
) -> Vec<Line<'static>> {
    let text_width = if user_msg.wall_clock.is_some() {
        content_width
            .saturating_sub(
                USER_TIMESTAMP_RESERVED_WIDTH.saturating_add(USER_TIMESTAMP_RIGHT_PADDING_WIDTH),
            )
            .max(1)
    } else {
        content_width
    };
    assemble_user_surface_lines(
        user_msg,
        theme,
        content_width,
        text_width,
        surface,
        agent_accent,
        body_style,
    )
}

fn assemble_user_surface_lines(
    user_msg: &TranscriptUserMessageSection,
    theme: &Theme,
    content_width: u16,
    text_width: u16,
    surface: Color,
    agent_accent: Color,
    body_style: Style,
) -> Vec<Line<'static>> {
    let mut lines = vec![user_surface_line(
        TRANSCRIPT_USER_BODY_PREFIX,
        Vec::new(),
        body_style,
        surface,
    )];
    let body_start = lines.len();
    append_user_surface_text_block(
        &mut lines,
        &user_msg.text,
        theme.text.primary,
        TRANSCRIPT_USER_BODY_PREFIX,
        text_width,
        surface,
    );
    collapse_user_surface_body(&mut lines, body_start, text_width, surface, body_style);
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
            crate::theme::GlyphMode::Preferred => "❯",
            crate::theme::GlyphMode::Ascii => ">",
        };
        let marker_prefix = format!("   {marker} ");
        lines[1].spans[0] = surface_span(
            marker_prefix,
            Style::default().fg(theme.markdown.text),
            surface,
        );
        if let Some(clock) = user_msg.wall_clock.as_deref() {
            let displayed_clock = if user_msg.wall_clock_hovered {
                user_msg.expanded_wall_clock.as_deref().unwrap_or(clock)
            } else {
                clock
            };
            append_user_row_wall_clock(
                &mut lines[1],
                displayed_clock,
                content_width,
                theme,
                surface,
            );
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

fn append_user_row_wall_clock(
    line: &mut Line<'static>,
    clock: &str,
    content_width: u16,
    theme: &Theme,
    surface: Color,
) {
    let clock_color = if surface == theme.surface.selected_card
        || surface == theme.terminal_colors.active_prompt_surface
    {
        theme.text.primary
    } else {
        theme.text.secondary
    };
    let timestamp = format!("  {clock}");
    let clock_width = display_width(&timestamp);
    let target = usize::from(content_width.saturating_sub(USER_TIMESTAMP_RIGHT_PADDING_WIDTH));
    if clock_width == 0 || target <= clock_width.saturating_add(1) {
        return;
    }
    let timestamp_start = target.saturating_sub(clock_width);
    truncate_line_to_width(line, timestamp_start);
    let used = line
        .spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum::<usize>();
    let pad = timestamp_start.saturating_sub(used);
    if pad > 0 {
        line.spans
            .push(surface_span(" ".repeat(pad), Style::default(), surface));
    }
    line.spans.push(surface_span(
        timestamp,
        Style::default().fg(clock_color),
        surface,
    ));
}

pub(super) fn truncate_line_to_width(line: &mut Line<'static>, width: usize) {
    let mut remaining = width;
    let mut clipped = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        if remaining == 0 {
            break;
        }
        let span_width = display_width(span.content.as_ref());
        if span_width <= remaining {
            clipped.push(span.clone());
            remaining = remaining.saturating_sub(span_width);
            continue;
        }
        let prefix = take_grapheme_width_prefix(span.content.as_ref(), remaining);
        if !prefix.is_empty() {
            clipped.push(Span::styled(prefix, span.style));
        }
        break;
    }
    line.spans = clipped;
}

fn assistant_clock_target_width(content_width: u16) -> usize {
    usize::from(content_width.saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH))
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
            glyph_routed_streaming_spinner_frame(theme, turn.animation_phase, turn.motion_enabled),
            agent_accent,
            "active",
        ),
        ActivityStatus::Done => ("", agent_accent, "done"),
        ActivityStatus::Error => (theme.live_shell.glyphs.error, theme.status.error, "error"),
    };

    let mut surfaces = Vec::new();
    for entry in assistant_visual_entry_plan(turn) {
        match entry {
            AssistantVisualEntryPlan::ToolGroup(group) => {
                surfaces.push(build_context_tool_group_render_surface(
                    turn,
                    &group,
                    theme,
                    width,
                    base_surface,
                ));
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
    let target = assistant_clock_target_width(content_width);
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
    let (
        kind,
        show_outer_rail,
        rail_color,
        surface,
        interaction_rows,
        selection_rows,
        diff_hunk_offsets,
        tool_rail_motion,
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
            let block_layout = append_reasoning_block(
                &mut lines,
                reasoning_text,
                ReasoningBlockContext {
                    theme,
                    width: transcript_surface_content_width(width, false),
                    surface: base_surface,
                    duration_ms,
                    active: reasoning_active,
                    expanded,
                    selected: turn.header.is_selected,
                    hovered: turn.header.is_hovered,
                },
            );
            let selection_rows = reasoning_selection_rows(&lines, width, block_layout);
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
                reasoning_active || expanded,
                theme.terminal_colors.muted,
                base_surface,
                Some(interaction_rows),
                Some(selection_rows),
                Vec::new(),
                (reasoning_active && motion_enabled).then_some(ToolRailMotion::Running {
                    elapsed: Duration::from_millis(
                        u64::try_from(turn.animation_phase)
                            .unwrap_or(u64::MAX)
                            .saturating_mul(crate::scheduling::active_animation_period_ms()),
                    ),
                    sampled_phase: turn.animation_phase,
                }),
            )
        }
        TranscriptAssistantPart::Body(_) => {
            let content_width = transcript_surface_content_width(width, false);
            let spec = super::ui_transcript_block_grammar::normalized_part_spec(turn, part_index);
            let content = resolve_assistant_body_content(&spec, theme, content_width);
            lines = content.lines;
            (
                TranscriptRenderSurfaceKind::AssistantBody,
                false,
                assistant_primary_rail_color(turn.header.status, &turn.header.profile_label, theme),
                base_surface,
                None,
                content.selection_rows,
                Vec::new(),
                None,
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
            let render_width = transcript_surface_render_width(
                width.saturating_sub(policy.trailing_gap_cells),
                kind,
            );
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
                tool_call.details_visible()
                    && ((family != TranscriptToolFamily::Unknown
                        && !super::super::ui_tool_titles::is_mcp_tool_id(
                            &tool_call.header.tool_id,
                        ))
                        || tool_call.expanded
                        || tool_call.details_preview_visible)
                    && (matches!(
                        family,
                        TranscriptToolFamily::Execute
                            | TranscriptToolFamily::Web
                            | TranscriptToolFamily::Unknown
                    ) || super::super::ui_tool_titles::is_mcp_tool_id(
                        &tool_call.header.tool_id,
                    ))
                    && !tool_call_is_todo(tool_call)
                    && tool_call.header.visual_style != TranscriptToolCallVisualStyle::TaskInline,
                tool_section_rail_color(tool_call, family, theme),
                base_surface,
                Some(render.interaction_rows),
                None,
                render.diff_hunk_offsets,
                match tool_call.rail_motion {
                    ToolRailMotion::Queued | ToolRailMotion::Settled => None,
                    motion => Some(motion),
                },
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
                None,
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
                None,
            )
        }
    };

    let mut interaction_rows = interaction_rows;
    let mut selection_rows = selection_rows;

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
        trailing_gap_rows: 0,
        placement: spec.placement,
        show_outer_rail,
        rail_glyph: if show_outer_rail || tool_rail_motion.is_some() {
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
    } = &spec.content
    else {
        return AssistantBodyContent {
            lines: Vec::new(),
            selection_rows: None,
        };
    };
    // Grok reserves a timestamp gutter for every message row and overlays the
    // clock on the first content row. A tool or a new paragraph must not add a
    // timestamp-only row above text that is already on screen.
    let body_width = content_width.saturating_sub(if wall_clock.is_some() { 10 } else { 0 });
    let mut lines = Vec::new();
    let mut selection_rows = if *streaming
        && wall_clock.is_none()
        && !text.contains("```")
        && !text.contains("~~~")
        && !text.contains("](")
        && !text.contains("http://")
        && !text.contains("https://")
    {
        None
    } else {
        selection_rows_for_rich_text_block(
            text,
            theme.markdown.text,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            theme,
            body_width,
            *streaming,
        )
    };
    if *streaming {
        append_streaming_rich_text_block(
            &mut lines,
            text,
            theme.markdown.text,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            theme,
            body_width,
        );
    } else {
        append_rich_text_block(
            &mut lines,
            text,
            theme.markdown.text,
            TRANSCRIPT_ASSISTANT_BODY_PREFIX,
            theme,
            body_width,
        );
    }
    while lines
        .last()
        .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.pop();
    }
    if let (Some(clock), Some(line)) = (wall_clock.as_deref(), lines.first_mut()) {
        pack_wall_clock_on_line(line, clock, content_width, theme);
    }
    if let Some(rows) = &mut selection_rows {
        rows.truncate(lines.len());
        for row in rows {
            row.cells.resize(usize::from(content_width), String::new());
        }
    }
    AssistantBodyContent {
        lines,
        selection_rows,
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
        trailing_gap_rows: 0,
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

struct ReasoningBlockContext<'a> {
    theme: &'a Theme,
    width: u16,
    surface: Color,
    duration_ms: Option<u64>,
    active: bool,
    expanded: bool,
    selected: bool,
    hovered: bool,
}

#[derive(Clone, Default)]
struct ReasoningBlockLayout {
    body_start: Option<usize>,
    selection_rows: Vec<Option<TranscriptSelectionRow>>,
}

fn append_reasoning_block(
    lines: &mut Vec<Line<'static>>,
    thinking_text: &str,
    context: ReasoningBlockContext<'_>,
) -> ReasoningBlockLayout {
    let ReasoningBlockContext {
        theme,
        width,
        surface,
        duration_ms,
        active,
        expanded,
        selected,
        hovered,
    } = context;
    let header_color = thinking_header_color(theme, surface);
    let muted_style = |color| {
        let style = Style::default().fg(color);
        if color == Color::Reset {
            style.add_modifier(Modifier::DIM)
        } else {
            style
        }
    };
    let prefix_style = Style::default().fg(header_color);
    let header_style = muted_style(header_color);
    let label_style = if selected {
        Style::default()
            .fg(theme.text.primary)
            .add_modifier(Modifier::BOLD)
    } else {
        muted_style(header_color).add_modifier(Modifier::BOLD)
    };

    let (title, body) = reasoning_summary(thinking_text);
    if title.is_none() && body.trim().is_empty() {
        return ReasoningBlockLayout::default();
    }

    let completed = !active;
    let marker_color = if active {
        theme.terminal_colors.muted
    } else if expanded {
        theme.text.tertiary
    } else {
        header_color
    };
    let mut header_spans = vec![Span::styled(
        format!(
            "{} ",
            if (selected || hovered) && completed && !expanded {
                if theme.glyph_mode() == crate::theme::GlyphMode::Ascii {
                    ">"
                } else {
                    "›"
                }
            } else {
                theme.live_shell.transcript_glyphs.tool_marker
            }
        ),
        Style::default().fg(marker_color),
    )];
    if completed {
        header_spans.push(Span::styled("Thought", label_style));
        if let Some(duration_ms) = duration_ms {
            header_spans.push(Span::styled(
                format!(" for {}", format_thought_duration_ms(duration_ms)),
                header_style,
            ));
        }
    } else {
        header_spans.push(Span::styled("Thinking…", label_style));
    }
    let content_prefix = "   ";
    append_prefixed_wrapped_spans_line(lines, content_prefix, prefix_style, header_spans, width);

    if completed && !expanded {
        return ReasoningBlockLayout::default();
    }

    let body = reasoning_body_text(thinking_text);
    if body.trim().is_empty() {
        return ReasoningBlockLayout::default();
    }

    let mut body_lines = Vec::new();
    // Grok wraps before taking the preview tail and reserves the block's
    // right padding. Counting wider rows would retain different old headers.
    let mut body_selection = super::ui_reasoning_markdown_body::append_reasoning_body_lines(
        &mut body_lines,
        &body,
        theme,
        surface,
        content_prefix,
        width
            .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
            .max(1),
    );
    let preview_inserted = !completed && !expanded && body_lines.len() > 3;
    let selection_rows = if preview_inserted {
        let preview_start = body_lines.len() - 3;
        let mut preview = vec![Line::from(vec![
            Span::raw(content_prefix.to_string()),
            Span::styled("…", muted_style(theme.text.secondary)),
        ])];
        preview.extend(body_lines.drain(preview_start..));
        body_lines = preview;
        let mut preview_selection = vec![None];
        preview_selection.extend(body_selection.drain(preview_start..).map(Some));
        preview_selection
    } else {
        body_selection.into_iter().map(Some).collect()
    };
    lines.push(Line::default());
    let body_start = lines.len();
    lines.extend(body_lines);
    ReasoningBlockLayout {
        body_start: Some(body_start),
        selection_rows,
    }
}

fn reasoning_selection_rows(
    lines: &[Line<'static>],
    width: u16,
    layout: ReasoningBlockLayout,
) -> Vec<TranscriptSelectionRow> {
    lines
        .iter()
        .enumerate()
        .map(|(index, _line)| {
            let Some(metadata) = layout
                .body_start
                .and_then(|start| index.checked_sub(start))
                .and_then(|body_index| layout.selection_rows.get(body_index))
                .cloned()
                .flatten()
            else {
                return blank_selection_row(width);
            };
            metadata
        })
        .collect()
}

fn reasoning_body_text(raw: &str) -> String {
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
    group: &super::ui_transcript_groups::TranscriptToolGroup,
    theme: &Theme,
    width: u16,
    base_surface: Color,
) -> TranscriptVisualEntryDraft {
    let tools = group
        .members
        .iter()
        .filter_map(|&index| turn.assistant_parts[index].tool_call())
        .collect::<Vec<_>>();
    let expanded = group.summary.disclosure == TranscriptToolDisclosureMode::Expanded;
    let context_group = group.summary.kind == TranscriptToolGroupKind::Context;
    let described = group
        .members
        .iter()
        .filter(|index| context_group || expanded || group.hidden.binary_search(index).is_ok())
        .filter_map(|&index| turn.assistant_parts[index].tool_call())
        .collect::<Vec<_>>();
    let label = TranscriptToolGroupSummary::from_tool_calls(&described);
    let failed = label
        .as_ref()
        .is_some_and(|summary| summary.failed_count > 0)
        || described
            .iter()
            .flat_map(|tool| &tool.hook_executions)
            .any(|hook| hook.status == harness_core::event::HookExecutionStatus::Failed);
    let active = context_group && group.summary.running_count + group.summary.queued_count > 0;
    let color = if failed && context_group {
        theme.terminal_colors.error
    } else if active {
        theme.text.tertiary
    } else {
        theme.text.secondary
    };
    let mut spans = vec![
        Span::raw(TRANSCRIPT_ASSISTANT_BODY_PREFIX),
        Span::styled(
            format!("{} ", theme.live_shell.transcript_glyphs.group_marker),
            Style::default().fg(color),
        ),
    ];
    let mut label_spans = vec![Span::styled(
        label.as_ref().map_or_else(
            || format!("{} more", group.hidden.len()),
            TranscriptToolGroupSummary::semantic_core_label,
        ),
        Style::default()
            .fg(theme.text.tertiary)
            .add_modifier(Modifier::BOLD),
    )];
    if let Some(summary) = label.filter(|summary| summary.failed_count > 0) {
        label_spans.push(Span::styled(
            format!(" · {} failed", summary.failed_count),
            Style::default().fg(theme.terminal_colors.error),
        ));
    }
    let hook_spans = if expanded {
        Vec::new()
    } else {
        super::ui_transcript_tool_hooks::summary_spans(
            described.iter().flat_map(|tool| &tool.hook_executions),
            true,
            theme,
        )
    };
    let content_width = usize::from(transcript_surface_content_width(width, false));
    let label_budget = content_width
        .saturating_sub(spans.iter().map(Span::width).sum())
        .saturating_sub(hook_spans.iter().map(Span::width).sum());
    let mut label_line = Line::from(label_spans);
    truncate_line_to_width(&mut label_line, label_budget);
    spans.extend(label_line.spans);
    spans.extend(hook_spans);
    let mut line = Line::from(spans);
    truncate_line_to_width(
        &mut line,
        usize::from(transcript_surface_content_width(width, false)),
    );
    let target = TranscriptMouseTarget::ToolGroup {
        tool_call_ids: group.target_ids.clone(),
    };
    if tools.first().and_then(|tool| tool.hovered_target.as_ref()) == Some(&target) {
        super::ui_transcript_tool_render::apply_header_hover(&mut line, expanded, theme);
    }
    TranscriptVisualEntryDraft {
        kind: TranscriptRenderSurfaceKind::AssistantTool,
        leading_gap_rows: 0,
        trailing_gap_rows: 0,
        placement: TranscriptBlockPlacement::Flow,
        show_outer_rail: false,
        rail_glyph: theme.live_shell.transcript_glyphs.rail,
        rail_color: color,
        surface: base_surface,
        lines: vec![line],
        interaction_rows: Some(vec![Some(full_width_interaction_row(target))]),
        selection_rows: None,
        diff_hunk_offsets: Vec::new(),
        selected_rail: false,
        tool_rail_motion: (active && !failed && turn.motion_enabled).then_some(
            ToolRailMotion::Running {
                elapsed: Duration::from_millis(
                    u64::try_from(turn.animation_phase)
                        .unwrap_or(u64::MAX)
                        .saturating_mul(crate::scheduling::active_animation_period_ms()),
                ),
                sampled_phase: turn.animation_phase,
            },
        ),
    }
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

fn tool_section_rail_color(
    tool: &TranscriptToolCallSection,
    family: TranscriptToolFamily,
    theme: &Theme,
) -> Color {
    match (family, tool.header.presentation.status) {
        (TranscriptToolFamily::Execute, ToolCallPresentationStatus::Queued)
            if matches!(tool.rail_motion, ToolRailMotion::Running { .. }) =>
        {
            theme.text.accent
        }
        (TranscriptToolFamily::Task, ToolCallPresentationStatus::Running) => {
            super::ui_transcript_style::blend_color(theme.surface.shell, theme.text.accent, 0.5)
        }
        (family, ToolCallPresentationStatus::Succeeded)
            if family != TranscriptToolFamily::Execute =>
        {
            theme.text.tertiary
        }
        (_, status) => tool_rail_color(status, theme),
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
    let marker = theme.live_shell.glyphs.pending_permission;
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
        let title = tool.header.title.trim();
        let label = if let Some(path) = title.strip_prefix("Creating ") {
            format!("Write `{path}`")
        } else if title.is_empty() {
            "tool".to_string()
        } else {
            title.to_string()
        };
        return Some(
            if matches!(tool.header.tool_id.as_str(), "bash" | "shell.run") {
                label
            } else {
                format!("Run {label}")
            },
        );
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
    use super::{build_transcript_render_surfaces, build_user_surface_lines, reasoning_summary};
    use crate::{
        app::{ActivityStatus, ActivityUsage, AppState, ToolCallDisplayStatus},
        theme::{GlyphMode, Theme},
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
            group: Default::default(),
            hook_executions: Vec::new(),
            tool_call_id: "edit-ui10".to_string(),
            coalesced_tool_call_ids: vec!["edit-ui10".to_string()],
            child_session_id: None,
            subagent_background: false,
            output_truncated: false,
            replay_read_only: false,
            hovered_target: None,
            header: super::super::TranscriptToolCallHeader {
                selected: false,
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
                    before_source: None,
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
                is_hovered: false,
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
                expanded_wall_clock: None,
                wall_clock_hovered: false,
            }),
            show_footer: false,
            footer_timestamp: None,
            animation_phase: 0,
            motion_enabled: false,
            reasoning_expanded: false,
            header: super::super::TranscriptTurnHeader {
                status: ActivityStatus::Done,
                is_selected: true,
                is_hovered: false,
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
        // arrange
        // act
        let theme = Theme::default();
        let surfaces = build_transcript_render_surfaces(
            &selected_user_turn(),
            &theme,
            80,
            theme.surface.shell,
        );

        // assert
        assert_eq!(surfaces[0].surface, theme.surface.selected_card);
        assert!(surfaces[0]
            .lines
            .iter()
            .flat_map(|line| &line.spans)
            .all(|span| span.style.bg == Some(theme.surface.selected_card)));
    }

    #[test]
    fn active_user_prompt_keeps_the_idle_surface() {
        // arrange
        let theme = Theme::default();

        // act
        let active =
            super::user_prompt_surface(&theme, super::TranscriptPromptState::ActiveThinking);
        let idle = super::user_prompt_surface(&theme, super::TranscriptPromptState::Idle);
        let selected = super::user_prompt_surface(&theme, super::TranscriptPromptState::Selected);

        // assert
        assert_eq!(active, idle);
        assert_eq!(selected, theme.surface.selected_card);
    }

    #[test]
    fn narrow_user_row_suppresses_timestamp_without_overflow() {
        // arrange
        // act
        let theme = Theme::default();
        let width = 11;
        let lines = build_user_surface_lines(
            &super::super::TranscriptUserMessageSection {
                text: "界".to_string(),
                queued: false,
                wall_clock: Some("12:34 PM".to_string()),
                expanded_wall_clock: Some("12:34:56 | Aug 14".to_string()),
                wall_clock_hovered: false,
            },
            &theme,
            width,
            theme.surface.card,
            theme.text.accent,
            ratatui::style::Style::default().fg(theme.text.primary),
        );

        // assert
        assert!(lines.iter().all(|line| line.width() <= usize::from(width)));
        assert!(lines
            .iter()
            .all(|line| !line.spans.iter().any(|span| span.content.contains("12:34"))));
    }

    #[test]
    fn syntax_style_upgrade_preserves_diff_text_rows_selection_and_anchors() {
        // arrange
        // Given: one expanded diff measured while its tool is still using provisional styles.
        let theme = Theme::default();
        let width = 36;
        for plain_numbered in [false, true] {
            let mut before_turn = ui10_diff_turn(false);
            let blocks = before_turn
                .assistant_parts
                .iter_mut()
                .filter_map(|part| match part {
                    super::super::TranscriptAssistantPart::ToolCall(tool) => {
                        Some(&mut tool.detail_blocks)
                    }
                    _ => None,
                })
                .flatten();
            for block in blocks {
                if let super::super::TranscriptToolCallDetailBlock::StructuredDiff {
                    plain_numbered: numbered,
                    ..
                } = block
                {
                    *numbered = plain_numbered;
                }
            }
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
            let before_selection =
                super::super::transcript_selection_rows(&before, usize::from(width));

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
            let after_selection =
                super::super::transcript_selection_rows(&after, usize::from(width));
            assert_ne!(
                after.sections[0].surfaces[0].lines,
                before.sections[0].surfaces[0].lines
            );

            // act
            // Then: style-only promotion leaves every geometry-bearing projection unchanged.
            // assert
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
    }

    #[test]
    fn active_lifecycle_has_one_status_source_and_stable_composer_geometry() {
        // arrange
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

        // act
        // Then: only the bottom-dock status owns active lifecycle text and the composer never moves.
        // assert
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
        // arrange
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

        // act
        for surface in surfaces.into_iter().filter(|surface| {
            matches!(
                surface.kind,
                super::super::TranscriptRenderSurfaceKind::AssistantReasoning
                    | super::super::TranscriptRenderSurfaceKind::AssistantBody
            )
        }) {
            // assert
            assert_eq!(
                surface.surface,
                ratatui::style::Color::Rgb(1, 2, 3),
                "ordinary assistant prose must use the themed base surface"
            );
        }
    }

    #[test]
    fn assistant_footer_only_surface_uses_themed_base_surface() {
        // arrange
        let mut app = AppState::default();
        app.activities = std::collections::VecDeque::from(vec![
            super::super::transcript_section_model_test_activity(
                "request-transparent-assistant-footer",
                ActivityStatus::Streaming,
                "",
            ),
        ]);
        app.transcript_view.selected_activity_index = 0;

        // act
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

        // assert
        assert_eq!(footer.surface, ratatui::style::Color::Rgb(1, 2, 3));
    }

    #[test]
    fn initial_provider_attempt_does_not_render_retry_chrome() {
        // arrange
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

        // act
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

        // assert
        assert!(text.is_empty(), "{text}");
    }

    #[test]
    fn command_group_failure_color_stays_on_the_failed_member() {
        // arrange
        // Given: adjacent commands with one successful and one failed member.
        let command = |id: &str, title: &str, status: ToolCallDisplayStatus| {
            super::super::TranscriptToolCallSection {
                group: Default::default(),
                hook_executions: Vec::new(),
                tool_call_id: id.to_string(),
                coalesced_tool_call_ids: vec![id.to_string()],
                child_session_id: None,
                subagent_background: false,
                output_truncated: false,
                replay_read_only: false,
                hovered_target: None,
                header: super::super::TranscriptToolCallHeader {
                    selected: false,
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
                is_hovered: false,
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
            assistant_parts: (0..10)
                .map(|index| {
                    super::super::TranscriptAssistantPart::ToolCall(Box::new(command(
                        &format!("prelude-{index}"),
                        &format!("printf prelude-{index}"),
                        ToolCallDisplayStatus::Succeeded,
                    )))
                })
                .chain([
                    super::super::TranscriptAssistantPart::ToolCall(Box::new(succeeded)),
                    super::super::TranscriptAssistantPart::ToolCall(Box::new(failed)),
                ])
                .collect(),
            assistant_part_source_ids: (1..=12)
                .map(super::super::TranscriptAssistantPartSourceId)
                .collect(),
        };

        // When: the mixed-status group is rendered.
        let theme = Theme::default();
        let entries =
            build_transcript_render_surfaces(&turn, &theme, 120, ratatui::style::Color::Reset);
        let lines = entries
            .iter()
            .flat_map(|entry| &entry.lines)
            .collect::<Vec<_>>();
        let row_has_error_color = |needle: &str| {
            let row = lines.iter().find(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .contains(needle)
            });
            assert!(row.is_some(), "missing {needle}: {lines:#?}");
            row.expect("command member row")
                .spans
                .iter()
                .any(|span| span.style.fg == Some(theme.status.error))
        };

        // act
        // Then: aggregate failure chrome does not recolor successful siblings.
        // assert
        assert!(!row_has_error_color("echo ok"));
        assert!(row_has_error_color("echo fail"));
    }

    #[test]
    fn assistant_tool_surfaces_use_themed_base_surface() {
        // arrange
        fn tool_section(
            id: &str,
            tool_id: &str,
            details: Vec<super::super::TranscriptToolCallDetailBlock>,
        ) -> super::super::TranscriptToolCallSection {
            super::super::TranscriptToolCallSection {
                group: Default::default(),
                hook_executions: Vec::new(),
                tool_call_id: id.to_string(),
                coalesced_tool_call_ids: vec![id.to_string()],
                child_session_id: None,
                subagent_background: false,
                output_truncated: false,
                replay_read_only: false,
                hovered_target: None,
                header: super::super::TranscriptToolCallHeader {
                    selected: false,
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
                is_hovered: false,
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

        // act
        for surface in surfaces.into_iter().filter(|surface| {
            matches!(
                surface.kind,
                super::super::TranscriptRenderSurfaceKind::AssistantTool
                    | super::super::TranscriptRenderSurfaceKind::AssistantCommandTool
            )
        }) {
            // assert
            assert_eq!(surface.surface, ratatui::style::Color::Rgb(1, 2, 3));
            let backgrounds = surface
                .lines
                .iter()
                .flat_map(|line| line.spans.iter())
                .map(|span| span.style.bg)
                .collect::<Vec<_>>();
            assert!(
                backgrounds.iter().all(|background| {
                    background.is_none()
                        || *background == Some(ratatui::style::Color::Rgb(1, 2, 3))
                        || *background == Some(theme.markdown.code_background)
                }),
                "{backgrounds:?}"
            );
        }
    }

    #[test]
    fn settled_provider_failure_uses_error_role_on_themed_base_surface() {
        // arrange
        let mut app = AppState::default();
        let mut entry = super::super::transcript_section_model_test_activity(
            "request-transparent-assistant-error",
            ActivityStatus::Error,
            "",
        );
        entry.error_message = Some("network unavailable".to_string());
        app.activities = std::collections::VecDeque::from(vec![entry]);
        app.transcript_view.selected_activity_index = 0;

        // act
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

        // assert
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

    #[test]
    fn dense_command_group_renders_exact_hidden_member_affordance() {
        // arrange
        let mut activity = super::super::transcript_section_model_test_activity(
            "request-dense-command-group",
            ActivityStatus::Done,
            "",
        );
        activity.tool_calls = (0_u64..12)
            .map(|index| {
                let mut tool = super::super::transcript_section_model_test_tool_call(
                    &format!("dense-command-{index}"),
                    "bash",
                );
                tool.status = ToolCallDisplayStatus::Succeeded;
                tool.args_summary = format!(r#"{{"command":"echo {index}"}}"#);
                tool.output_summary = Some(format!("command {index} complete"));
                tool.first_seq = index.saturating_add(1);
                tool.last_seq = index.saturating_add(1);
                tool
            })
            .collect();
        let mut app = AppState::default();
        app.activities.push_back(activity);
        let sections = super::super::build_transcript_sections(&app);
        let theme = Theme::default();

        // act
        let surfaces = build_transcript_render_surfaces(
            &sections[0],
            &theme,
            120,
            ratatui::style::Color::Reset,
        );
        let affordance = surfaces
            .iter()
            .flat_map(|surface| surface.lines.iter())
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .find(|line| line.contains("Ran 2 commands"))
            .expect("dense fold affordance");

        // assert
        assert_eq!(
            affordance.trim(),
            format!(
                "{} Ran 2 commands",
                theme.live_shell.transcript_glyphs.group_marker
            )
        );
        app.activities[0].tool_calls.truncate(11);
        let sections = super::super::build_transcript_sections(&app);
        let surfaces = build_transcript_render_surfaces(
            &sections[0],
            &theme,
            120,
            ratatui::style::Color::Reset,
        );
        let text = surfaces
            .iter()
            .flat_map(|surface| &surface.lines)
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(!text.contains("more") && !text.contains("Ran 11"), "{text}");
        assert!(text.contains("echo 0") && text.contains("echo 10"));
    }

    #[test]
    fn expanded_context_group_preserves_each_path_and_target() {
        // Given: two expanded reads whose identities must remain distinct.
        let mut turn = ui10_diff_turn(false);
        let template = turn.assistant_parts[0].clone();
        turn.assistant_parts = [("first", "src/one.rs"), ("second", "src/two.rs")]
            .into_iter()
            .map(|(id, path)| {
                let mut part = template.clone();
                if let super::super::TranscriptAssistantPart::ToolCall(tool) = &mut part {
                    tool.tool_call_id = id.to_string();
                    tool.coalesced_tool_call_ids = vec![id.to_string()];
                    tool.header.tool_id = "read".to_string();
                    tool.header.title = "Read".to_string();
                    tool.header.path_metadata = Some(path.to_string());
                    tool.header.disclosure_state =
                        Some(super::super::TranscriptToolCallDisclosureState::Expanded);
                    tool.expanded = true;
                    tool.detail_blocks.clear();
                }
                part
            })
            .collect();
        turn.assistant_part_source_ids = vec![
            super::super::TranscriptAssistantPartSourceId(1),
            super::super::TranscriptAssistantPartSourceId(2),
        ];

        // When: the grouped surface is measured and rendered.
        let surfaces = build_transcript_render_surfaces(
            &turn,
            &Theme::default(),
            80,
            ratatui::style::Color::Reset,
        );
        let text = surfaces
            .iter()
            .flat_map(|surface| &surface.lines)
            .flat_map(|line| &line.spans)
            .map(|span| span.content.as_ref())
            .collect::<String>();

        // Then: neither file identity is replaced by the aggregate count.
        assert!(
            text.contains("Read src/one.rs") && text.contains("Read src/two.rs"),
            "{text}"
        );
    }

    #[test]
    fn completion_rail_covers_header_without_changing_content_or_geometry() {
        let theme = Theme::default();
        let mut turn = ui10_diff_turn(true);
        let settled = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.canvas);
        if let super::super::TranscriptAssistantPart::ToolCall(tool) = &mut turn.assistant_parts[0]
        {
            tool.rail_motion = super::super::ToolRailMotion::FinishFlash {
                elapsed: std::time::Duration::ZERO,
                sampled_phase: 0,
            };
        }
        let flashed = build_transcript_render_surfaces(&turn, &theme, 80, theme.surface.canvas);
        assert_eq!(settled[0].lines.len(), flashed[0].lines.len());
        assert_eq!(
            settled[0].lines[0].spans[1..],
            flashed[0].lines[0].spans[1..]
        );
        assert!(super::super::ui_transcript_surface::line_has_tool_rail(
            &flashed[0].lines[0],
            theme.live_shell.transcript_glyphs.rail,
        ));
        let rails = flashed[0]
            .lines
            .iter()
            .filter(|line| {
                super::super::ui_transcript_surface::line_has_tool_rail(
                    line,
                    theme.live_shell.transcript_glyphs.rail,
                )
            })
            .collect::<Vec<_>>();
        assert!(!rails.is_empty());
        assert!(rails
            .iter()
            .all(|line| line.spans[0].style.fg == Some(theme.status.success)));
        for (before, after) in settled[0].lines.iter().zip(&flashed[0].lines) {
            assert_eq!(before.width(), after.width());
        }
    }
}
