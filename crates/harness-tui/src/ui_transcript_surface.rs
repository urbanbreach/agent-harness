// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::design_contract::{MotionKind, DESIGN_TOKENS};
use crate::theme::Theme;

use super::ui_chrome::{display_width, panel_style, take_width_prefix};
use super::ui_transcript::{ToolRailMotion, TranscriptRenderSurface, TranscriptRenderSurfaceKind};
use super::ui_transcript_layout::MeasuredTranscriptSurface;
use super::ui_transcript_style::{
    blend_color, pending_diamond_color, transcript_running_tool_marker_color,
    transcript_streaming_spinner_frame,
};

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 1;
pub(super) const TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH: u16 = 2;
pub(super) const TRANSCRIPT_RAIL_GLYPH: &str = " ";

pub(super) fn transcript_surface_leading_gap(
    previous: Option<TranscriptRenderSurfaceKind>,
    current: TranscriptRenderSurfaceKind,
) -> usize {
    match previous {
        Some(previous)
            if transcript_surface_is_assistant_tool_like(previous)
                && transcript_surface_is_assistant_tool_like(current) =>
        {
            1
        }
        // Reference question state: Thought then Ask are adjacent (no blank between).
        Some(TranscriptRenderSurfaceKind::AssistantReasoning)
            if transcript_surface_is_assistant_tool_like(current) =>
        {
            0
        }
        Some(TranscriptRenderSurfaceKind::AssistantBody)
            if transcript_surface_is_assistant_tool_like(current) =>
        {
            1
        }
        Some(TranscriptRenderSurfaceKind::AssistantBody)
            if matches!(
                current,
                TranscriptRenderSurfaceKind::AssistantReasoning
                    | TranscriptRenderSurfaceKind::AssistantBody
            ) =>
        {
            0
        }
        Some(previous)
            if transcript_surface_is_assistant_tool_like(previous)
                && current == TranscriptRenderSurfaceKind::AssistantReasoning =>
        {
            0
        }
        Some(_) => 1,
        None => 0,
    }
}

fn transcript_surface_is_assistant_tool_like(kind: TranscriptRenderSurfaceKind) -> bool {
    matches!(
        kind,
        TranscriptRenderSurfaceKind::AssistantTool
            | TranscriptRenderSurfaceKind::AssistantCommandTool
    )
}

pub(super) fn render_transcript_surface(
    frame: &mut Frame,
    surface: &MeasuredTranscriptSurface,
    area: Rect,
    local_scroll: usize,
    animation_phase: usize,
    theme: &Theme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface.surface)),
        area,
    );

    let tool_rail_overlay = surface.tool_rail_motion.is_some();
    let rail_visible = surface.show_outer_rail || tool_rail_overlay;
    let rail_width = if surface.show_outer_rail && !tool_rail_overlay {
        area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH)
    } else {
        0
    };
    if rail_visible && !tool_rail_overlay {
        let rail_rect = Rect::new(
            area.x,
            area.y,
            area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(transcript_surface_rail_lines_for_motion(
                surface,
                local_scroll,
                usize::from(area.height),
                animation_phase,
            ))
            .style(Style::default().bg(surface.surface)),
            rail_rect,
        );
    }

    if area.width <= rail_width {
        return;
    }

    let content_rect = Rect::new(
        area.x.saturating_add(rail_width),
        area.y,
        area.width.saturating_sub(rail_width),
        area.height,
    );
    let mut visible_lines =
        visible_surface_lines(surface, local_scroll, usize::from(content_rect.height));
    apply_surface_animation_phase(&mut visible_lines, surface, animation_phase, theme);
    if surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning
        && surface.show_outer_rail
        && local_scroll == 0
    {
        if let Some(line) = visible_lines.first_mut() {
            apply_reasoning_spinner_phase(line, animation_phase);
        }
    }
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(panel_style(surface.surface, theme.text.primary));
    frame.render_widget(paragraph, content_rect);
    if tool_rail_overlay {
        let rail_rect = Rect::new(
            area.x,
            area.y,
            area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(transcript_surface_rail_lines_for_motion(
                surface,
                local_scroll,
                usize::from(area.height),
                animation_phase,
            ))
            .style(Style::default().bg(surface.surface)),
            rail_rect,
        );
    }
}

fn apply_surface_animation_phase(
    lines: &mut [Line<'static>],
    surface: &MeasuredTranscriptSurface,
    animation_phase: usize,
    theme: &Theme,
) {
    for line in lines {
        apply_reasoning_spinner_phase(line, animation_phase);
        for span in &mut line.spans {
            if matches!(
                span.content.as_ref(),
                "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧"
            ) {
                span.content = transcript_streaming_spinner_frame(animation_phase).into();
            }
            if matches!(
                surface.tool_rail_motion,
                Some(ToolRailMotion::Running { .. })
            ) && span.content.as_ref() == surface.rail_glyph
            {
                span.style = span
                    .style
                    .fg(transcript_running_tool_marker_color(theme, animation_phase));
            }
        }
        if surface.kind == TranscriptRenderSurfaceKind::User {
            if let Some(marker) = line
                .spans
                .iter_mut()
                .find(|span| span.content.as_ref() == "◆")
            {
                marker.style = marker
                    .style
                    .fg(pending_diamond_color(theme, animation_phase));
            }
        }
    }
}

pub(super) fn apply_reasoning_spinner_phase(line: &mut Line<'static>, animation_phase: usize) {
    let Some(marker_index) = line
        .spans
        .iter()
        .position(|span| span.content.as_ref() == "⠋")
    else {
        return;
    };
    let Some(marker) = line.spans.get_mut(marker_index) else {
        return;
    };
    let content = marker.content.to_mut();
    let prefix_len = content.len().saturating_sub(content.trim_start().len());
    let Some(first) = content[prefix_len..].chars().next() else {
        return;
    };
    let end = prefix_len.saturating_add(first.len_utf8());
    content.replace_range(
        prefix_len..end,
        transcript_streaming_spinner_frame(animation_phase),
    );
}

fn transcript_surface_rail_lines_for_motion(
    surface: &MeasuredTranscriptSurface,
    local_scroll: usize,
    visible_height: usize,
    animation_phase: usize,
) -> Vec<Line<'static>> {
    let dim = blend_color(Color::Rgb(0, 0, 0), surface.rail_color, 0.35);
    (0..visible_height)
        .map(|local_row| {
            let absolute_row = local_scroll.saturating_add(local_row);
            let color = match surface.tool_rail_motion {
                Some(ToolRailMotion::Running { .. }) => {
                    let pulse_phase = DESIGN_TOKENS
                        .motion_tokens
                        .all
                        .iter()
                        .find(|token| token.kind == MotionKind::ToolPulse)
                        .map_or(animation_phase, |token| {
                            let frames = usize::from(token.frames);
                            if frames == 0 {
                                animation_phase
                            } else {
                                animation_phase % frames
                            }
                        });
                    if surface.height <= 1 {
                        if pulse_phase.is_multiple_of(2) {
                            surface.rail_color
                        } else {
                            blend_color(Color::Rgb(0, 0, 0), surface.rail_color, 0.55)
                        }
                    } else if absolute_row == pulse_phase % surface.height {
                        surface.rail_color
                    } else {
                        dim
                    }
                }
                Some(ToolRailMotion::FinishFlash { .. }) => {
                    let alpha = if animation_phase.is_multiple_of(2) {
                        0.8
                    } else {
                        0.55
                    };
                    blend_color(surface.surface, Color::White, alpha)
                }
                Some(ToolRailMotion::Waiting)
                | Some(ToolRailMotion::Queued)
                | Some(ToolRailMotion::Settled)
                | None => surface.rail_color,
            };
            Line::from(Span::styled(
                surface.rail_glyph,
                Style::default().fg(color).bg(surface.surface),
            ))
        })
        .collect()
}

pub(super) fn visible_surface_lines(
    surface: &MeasuredTranscriptSurface,
    local_scroll: usize,
    visible_height: usize,
) -> Vec<Line<'static>> {
    if visible_height == 0 {
        return Vec::new();
    }

    surface
        .lines
        .iter()
        .skip(local_scroll)
        .take(visible_height)
        .cloned()
        .collect()
}

pub(super) fn render_transcript_surface_lines(
    surfaces: &[TranscriptRenderSurface],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut previous_surface_kind = None;
    for surface in surfaces {
        for _ in 0..transcript_surface_leading_gap(previous_surface_kind, surface.kind) {
            lines.push(Line::default());
        }
        if surface.show_outer_rail {
            lines.extend(surface.lines.iter().cloned().map(|line| {
                prepend_transcript_surface_rail(
                    line,
                    surface.rail_glyph,
                    surface.rail_color,
                    surface.surface,
                )
            }));
        } else {
            lines.extend(surface.lines.iter().cloned());
        }
        previous_surface_kind = Some(surface.kind);
    }
    lines
}

pub(super) fn transcript_surface_content_width(width: u16, show_outer_rail: bool) -> u16 {
    if show_outer_rail {
        width.saturating_sub(TRANSCRIPT_SURFACE_RAIL_WIDTH).max(1)
    } else {
        width.max(1)
    }
}

pub(super) fn transcript_surface_render_width(
    width: u16,
    kind: TranscriptRenderSurfaceKind,
) -> u16 {
    match kind {
        // User surfaces pack wall-clock on the first content row. A trailing gap of 2
        // drops content_width below freeze packing (e.g. "all names" + clock at 120x32
        // with dual gutter + scrollbar needs content_width >= 108).
        TranscriptRenderSurfaceKind::User => width.max(1),
        TranscriptRenderSurfaceKind::AssistantCommandTool
        | TranscriptRenderSurfaceKind::Compaction => width
            .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
            .max(1),
        _ => width.max(1),
    }
}

pub(super) fn append_prebuilt_nested_surface_lines(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let prefix = nested_surface_prefix(indent, rail_color, surface);
    let prefix_width = nested_surface_prefix_width(indent);
    for line in prebuilt {
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            line.spans,
            width,
            surface,
        ));
    }
}

pub(super) fn append_prebuilt_surface_lines(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let prefix = surface_prefix(indent);
    let prefix_width = surface_prefix_width(indent);
    for line in prebuilt {
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            line.spans,
            width,
            surface,
        ));
    }
}

pub(super) fn append_surface_row(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    surface: Color,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let prefix = surface_prefix(indent);
    let prefix_width = surface_prefix_width(indent);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wrapped_rows = wrap_surface_spans(content_spans, content_width);

    if wrapped_rows.is_empty() {
        lines.push(surface_line(
            prefix,
            prefix_width,
            Vec::new(),
            width,
            surface,
        ));
        return;
    }

    for row in wrapped_rows {
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            row,
            width,
            surface,
        ));
    }
}

pub(super) fn append_user_surface_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: Color,
    prefix: &str,
    width: u16,
    surface: Color,
) {
    append_user_surface_text_block_with_first_line_reserve(
        lines, text, color, prefix, width, surface, 0,
    );
}

pub(super) fn append_user_surface_text_block_with_first_line_reserve(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: Color,
    prefix: &str,
    width: u16,
    surface: Color,
    first_line_reserve: usize,
) {
    let base_style = Style::default().fg(color);
    let mut first_line = true;
    for line in text.lines() {
        let reserve = if first_line { first_line_reserve } else { 0 };
        first_line = false;
        append_user_surface_wrapped_line(
            lines,
            if line.is_empty() {
                Vec::new()
            } else {
                vec![Span::styled(line.to_string(), base_style)]
            },
            prefix,
            base_style,
            width,
            surface,
            reserve,
        );
    }

    if text.is_empty() {
        append_user_surface_wrapped_line(
            lines,
            Vec::new(),
            prefix,
            base_style,
            width,
            surface,
            first_line_reserve,
        );
    }
}

pub(super) fn append_user_surface_wrapped_line(
    lines: &mut Vec<Line<'static>>,
    content_spans: Vec<Span<'static>>,
    prefix: &str,
    prefix_style: Style,
    width: u16,
    surface: Color,
    first_row_reserve: usize,
) {
    let prefix_width = display_width(prefix);
    let full_content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let first_content_width = full_content_width.saturating_sub(first_row_reserve).max(1);
    if content_spans.is_empty() {
        lines.push(user_surface_line(prefix, Vec::new(), prefix_style, surface));
        return;
    }

    if first_row_reserve == 0 || first_content_width == full_content_width {
        for row in wrap_surface_spans(content_spans, full_content_width) {
            lines.push(user_surface_line(prefix, row, prefix_style, surface));
        }
        return;
    }

    let mut trimming_leading_whitespace = true;
    let content_spans = content_spans
        .into_iter()
        .filter_map(|mut span| {
            if !trimming_leading_whitespace {
                return Some(span);
            }
            let start = span
                .content
                .find(|character: char| !character.is_whitespace())?;
            trimming_leading_whitespace = false;
            if start > 0 {
                span.content = span.content[start..].to_string().into();
            }
            Some(span)
        })
        .collect::<Vec<_>>();
    let narrow_rows = wrap_surface_spans(content_spans.clone(), first_content_width);
    let Some(first) = narrow_rows.first().cloned() else {
        return;
    };
    let consumed_characters = first
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    lines.push(user_surface_line(prefix, first, prefix_style, surface));
    if narrow_rows.len() <= 1 {
        return;
    }
    let mut characters_to_skip = consumed_characters;
    let remainder_spans = content_spans
        .into_iter()
        .filter_map(|span| {
            let content = span.content.into_owned();
            let character_count = content.chars().count();
            if characters_to_skip >= character_count {
                characters_to_skip = characters_to_skip.saturating_sub(character_count);
                return None;
            }
            let remainder = content.chars().skip(characters_to_skip).collect::<String>();
            characters_to_skip = 0;
            Some(Span::styled(remainder, span.style))
        })
        .collect::<Vec<_>>();
    if remainder_spans.is_empty() {
        return;
    }
    for row in wrap_surface_spans(remainder_spans, full_content_width) {
        lines.push(user_surface_line(prefix, row, prefix_style, surface));
    }
}

pub(super) fn user_surface_line(
    prefix: &str,
    content_spans: Vec<Span<'static>>,
    prefix_style: Style,
    surface: Color,
) -> Line<'static> {
    let mut spans = vec![surface_span(prefix, prefix_style, surface)];
    for span in content_spans {
        spans.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    Line::from(spans)
}

pub(super) fn append_prefixed_wrapped_spans_line(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    prefix_style: Style,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    if content_spans.is_empty() {
        lines.push(Line::from(Span::styled(prefix.to_string(), prefix_style)));
        return;
    }

    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    for row in wrap_surface_spans(content_spans, content_width) {
        let mut spans = vec![Span::styled(prefix.to_string(), prefix_style)];
        spans.extend(row);
        lines.push(Line::from(spans));
    }
}

pub(super) fn append_prebuilt_plain_lines(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    for line in prebuilt {
        append_prefixed_wrapped_spans_line(lines, prefix, Style::default(), line.spans, width);
    }
}

fn surface_prefix(indent: &str) -> Vec<Span<'static>> {
    if indent.is_empty() {
        Vec::new()
    } else {
        vec![Span::raw(indent.to_string())]
    }
}

pub(super) fn surface_prefix_width(indent: &str) -> usize {
    display_width(indent)
}

fn surface_line(
    mut prefix: Vec<Span<'static>>,
    prefix_width: usize,
    content_spans: Vec<Span<'static>>,
    width: u16,
    surface: Color,
) -> Line<'static> {
    let mut visible_width = prefix_width;
    for span in content_spans {
        visible_width += span.width();
        prefix.push(surface_span(span.content.into_owned(), span.style, surface));
    }
    let remaining = usize::from(width).saturating_sub(visible_width);
    if remaining > 0 {
        prefix.push(surface_span(
            " ".repeat(remaining),
            Style::default(),
            surface,
        ));
    }
    Line::from(prefix)
}

pub(super) fn surface_span(text: impl Into<String>, style: Style, surface: Color) -> Span<'static> {
    Span::styled(text.into(), Style::default().bg(surface).patch(style))
}

pub(super) fn append_nested_surface_row(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    rail_color: Color,
    surface: Color,
    content_leading_spaces: &str,
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let prefix = nested_surface_prefix(indent, rail_color, surface);
    let prefix_width = nested_surface_prefix_width(indent);
    let leading_width = display_width(content_leading_spaces);
    let content_width = usize::from(width)
        .saturating_sub(prefix_width)
        .saturating_sub(leading_width)
        .max(1);
    let wrapped_rows = wrap_surface_spans(content_spans, content_width);

    if wrapped_rows.is_empty() {
        lines.push(surface_line(
            prefix,
            prefix_width,
            Vec::new(),
            width,
            surface,
        ));
        return;
    }

    let leading_span = if content_leading_spaces.is_empty() {
        None
    } else {
        Some(Span::styled(
            content_leading_spaces.to_string(),
            Style::default().bg(surface),
        ))
    };

    for row in wrapped_rows {
        let mut row = row;
        if let Some(leading) = leading_span.clone() {
            row.insert(0, leading);
        }
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            row,
            width,
            surface,
        ));
    }
}

fn nested_surface_prefix(indent: &str, rail_color: Color, surface: Color) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    if !indent.is_empty() {
        spans.push(Span::raw(indent.to_string()));
    }
    spans.push(Span::styled(
        TRANSCRIPT_RAIL_GLYPH,
        Style::default().fg(rail_color).bg(surface),
    ));
    spans.push(surface_span(" ", Style::default(), surface));
    spans
}

pub(super) fn nested_surface_prefix_width(indent: &str) -> usize {
    display_width(indent) + display_width(TRANSCRIPT_RAIL_GLYPH) + 1
}

pub(super) fn wrap_surface_spans(
    spans: Vec<Span<'static>>,
    width: usize,
) -> Vec<Vec<Span<'static>>> {
    if spans.is_empty() {
        return Vec::new();
    }

    let mut rows = Vec::new();
    let mut current = Vec::new();
    let mut current_width = 0;

    for token in spans.into_iter().flat_map(surface_wrap_tokens) {
        let token_text = token.content.to_string();
        let token_width = token.width();
        let token_is_whitespace = token_text.chars().all(char::is_whitespace);

        if token_is_whitespace && current.is_empty() {
            continue;
        }

        if current_width + token_width <= width {
            current_width += token_width;
            current.push(token);
            continue;
        }

        if token_is_whitespace {
            if !current.is_empty() {
                rows.push(current);
                current = Vec::new();
                current_width = 0;
            }
            continue;
        }

        if !current.is_empty() {
            rows.push(current);
            current = Vec::new();
            current_width = 0;
        }

        let mut remainder = token_text.as_str();
        while !remainder.is_empty() {
            let chunk = take_width_prefix(remainder, width);
            current_width = display_width(chunk);
            current.push(Span::styled(chunk.to_string(), token.style));
            remainder = &remainder[chunk.len()..];
            if !remainder.is_empty() {
                rows.push(current);
                current = Vec::new();
                current_width = 0;
            }
        }
    }

    if !current.is_empty() {
        rows.push(current);
    }

    rows
}

fn surface_wrap_tokens(span: Span<'static>) -> Vec<Span<'static>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut current_is_whitespace = None;

    for ch in span.content.chars() {
        let is_whitespace = ch.is_whitespace();
        if current_is_whitespace == Some(is_whitespace) || current.is_empty() {
            current.push(ch);
            current_is_whitespace = Some(is_whitespace);
            continue;
        }

        tokens.push(Span::styled(current.clone(), span.style));
        current.clear();
        current.push(ch);
        current_is_whitespace = Some(is_whitespace);
    }

    if !current.is_empty() {
        tokens.push(Span::styled(current, span.style));
    }

    tokens
}

fn transcript_surface_rail_lines(
    height: usize,
    rail_glyph: &'static str,
    rail_color: Color,
    surface: Color,
) -> Text<'static> {
    Text::from(
        (0..height)
            .map(|_| {
                Line::from(Span::styled(
                    rail_glyph,
                    Style::default().fg(rail_color).bg(surface),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

fn prepend_transcript_surface_rail(
    line: Line<'static>,
    rail_glyph: &'static str,
    rail_color: Color,
    surface: Color,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        rail_glyph,
        Style::default().fg(rail_color).bg(surface),
    )];
    spans.extend(line.spans);
    Line::from(spans)
}
