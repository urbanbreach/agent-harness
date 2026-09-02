// allow: SIZE_OK — TUI transcript rendering (indivisible view model)
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    Frame,
};
use std::borrow::Borrow;
use std::time::Duration;

use crate::theme::Theme;

use super::ui_chrome::{display_width, panel_style};
use super::ui_transcript::{
    ToolRailMotion, TranscriptRenderSurfaceKind, TranscriptVisualEntryDraft,
};
use super::ui_transcript_layout::TranscriptVisualEntry;
use super::ui_transcript_style::{
    blend_color, glyph_routed_streaming_spinner_frame, pending_diamond_color,
};
use crate::composer_atoms::split_graphemes;
use crate::terminal::char_display_width;

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 1;
pub(super) const TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH: u16 = 2;
pub(super) const TRANSCRIPT_RAIL_GLYPH: &str = " ";

pub(super) fn render_transcript_surface(
    frame: &mut Frame,
    surface: &TranscriptVisualEntry,
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
    let rail_width = 0;

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
    apply_surface_animation_phase(
        &mut visible_lines,
        surface,
        local_scroll,
        animation_phase,
        theme,
    );
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(panel_style(surface.surface, theme.text.primary));
    frame.render_widget(paragraph, content_rect);
    if rail_visible {
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
    surface: &TranscriptVisualEntry,
    local_scroll: usize,
    animation_phase: usize,
    theme: &Theme,
) {
    for (local_row, line) in lines.iter_mut().enumerate() {
        for span in &mut line.spans {
            if matches!(
                span.content.as_ref(),
                "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧"
            ) {
                span.content =
                    glyph_routed_streaming_spinner_frame(theme, animation_phase, true).into();
            }
        }
        let absolute_row = local_scroll.saturating_add(local_row);
        apply_tool_header_motion_color(line, surface, absolute_row, animation_phase, theme);
        if matches!(
            surface.kind,
            TranscriptRenderSurfaceKind::User | TranscriptRenderSurfaceKind::AssistantFooter
        ) {
            let marker_glyphs = [
                theme.live_shell.glyphs.pending_permission,
                theme.live_shell.transcript_glyphs.tool_marker,
                theme.live_shell.transcript_glyphs.thought_marker,
                theme.live_shell.transcript_glyphs.group_marker,
            ];
            if let Some(marker) = line
                .spans
                .iter_mut()
                .find(|span| marker_glyphs.contains(&span.content.trim()))
            {
                marker.style = marker
                    .style
                    .fg(pending_diamond_color(theme, animation_phase));
            }
        }
    }
}

#[cfg(test)]
mod animation_phase_tests {
    use super::{apply_surface_animation_phase, render_transcript_surface};
    use crate::theme::Theme;
    use crate::ui::ui_transcript::{
        ToolRailMotion, TranscriptBlockPlacement, TranscriptRenderSurfaceKind,
        TranscriptVisualEntryDisplayMode, TranscriptVisualEntryHitRegion,
        TranscriptVisualEntryMetadata,
    };
    use crate::ui::ui_transcript_layout::TranscriptVisualEntry;
    use ratatui::{backend::TestBackend, layout::Rect, style::Style, text::Span, Terminal};

    fn reasoning_surface(
        theme: &Theme,
        marker: &str,
        motion: Option<ToolRailMotion>,
    ) -> TranscriptVisualEntry {
        TranscriptVisualEntry {
            metadata: TranscriptVisualEntryMetadata::settled(
                0,
                0,
                TranscriptRenderSurfaceKind::AssistantReasoning,
                TranscriptVisualEntryDisplayMode::Flow,
            ),
            kind: TranscriptRenderSurfaceKind::AssistantReasoning,
            leading_gap_rows: 0,
            placement: TranscriptBlockPlacement::Flow,
            top_offset: 0,
            height: 1,
            width: 80,
            show_outer_rail: true,
            rail_glyph: "┃",
            rail_color: theme.text.tertiary,
            surface: theme.surface.canvas,
            lines: vec![ratatui::text::Line::from(vec![
                Span::raw("  "),
                Span::styled(
                    format!("{marker} "),
                    Style::default().fg(theme.text.tertiary),
                ),
                Span::styled(
                    "Thinking…",
                    Style::default()
                        .fg(theme.text.secondary)
                        .add_modifier(ratatui::style::Modifier::BOLD),
                ),
            ])],
            interaction_rows: None,
            selection_rows: None,
            diff_hunk_offsets: Vec::new(),
            selected_rail: false,
            tool_rail_motion: motion,
            hit_region: TranscriptVisualEntryHitRegion::new(0, 80, 1),
        }
    }

    #[test]
    fn cached_assistant_footer_rehydrates_the_pending_diamond_phase() {
        // arrange
        // Given: a cached waiting footer with independently styled marker and label spans.
        let theme = Theme::default();
        let surface = TranscriptVisualEntry {
            metadata: TranscriptVisualEntryMetadata::settled(
                0,
                0,
                TranscriptRenderSurfaceKind::AssistantFooter,
                TranscriptVisualEntryDisplayMode::Flow,
            ),
            kind: TranscriptRenderSurfaceKind::AssistantFooter,
            leading_gap_rows: 0,
            placement: TranscriptBlockPlacement::Flow,
            top_offset: 0,
            height: 1,
            width: 80,
            show_outer_rail: false,
            rail_glyph: " ",
            rail_color: theme.text.secondary,
            surface: theme.surface.canvas,
            lines: vec![ratatui::text::Line::from(vec![
                Span::raw("    "),
                Span::styled("◆ ", Style::default().fg(theme.text.secondary)),
                Span::styled(
                    "Waiting on answers",
                    Style::default().fg(theme.text.secondary),
                ),
            ])],
            interaction_rows: None,
            selection_rows: None,
            diff_hunk_offsets: Vec::new(),
            selected_rail: false,
            tool_rail_motion: None,
            hit_region: TranscriptVisualEntryHitRegion::new(0, 80, 1),
        };

        // When: cached lines are rehydrated at two runtime animation phases.
        let mut first = surface.lines.clone();
        apply_surface_animation_phase(&mut first, &surface, 0, 0, &theme);
        let mut later = surface.lines.clone();
        apply_surface_animation_phase(&mut later, &surface, 0, 10, &theme);

        // act
        // Then: the marker changes color while the waiting label remains muted.
        // assert
        assert_ne!(first[0].spans[1].style.fg, later[0].spans[1].style.fg);
        assert_eq!(first[0].spans[2].style.fg, Some(theme.text.secondary));
        assert_eq!(later[0].spans[2].style.fg, Some(theme.text.secondary));
    }

    #[test]
    fn active_reasoning_wave_animates_only_the_diamond() {
        // arrange
        let theme = Theme::default();
        let surface = reasoning_surface(
            &theme,
            "◆",
            Some(ToolRailMotion::Running {
                elapsed: std::time::Duration::ZERO,
                sampled_phase: 0,
            }),
        );

        // act
        let mut first = surface.lines.clone();
        apply_surface_animation_phase(&mut first, &surface, 0, 0, &theme);
        let mut later = surface.lines.clone();
        apply_surface_animation_phase(&mut later, &surface, 0, 10, &theme);

        // assert
        assert_ne!(first[0].spans[1].style.fg, later[0].spans[1].style.fg);
        assert_eq!(first[0].spans[2].style.fg, Some(theme.text.secondary));
        assert_eq!(later[0].spans[2].style.fg, Some(theme.text.secondary));
    }

    #[test]
    fn active_reasoning_wave_animates_the_ascii_marker() {
        // arrange
        let theme = Theme::default();
        let surface = reasoning_surface(
            &theme,
            "*",
            Some(ToolRailMotion::Running {
                elapsed: std::time::Duration::ZERO,
                sampled_phase: 0,
            }),
        );

        // act
        let mut first = surface.lines.clone();
        apply_surface_animation_phase(&mut first, &surface, 0, 0, &theme);
        let mut later = surface.lines.clone();
        apply_surface_animation_phase(&mut later, &surface, 0, 10, &theme);

        // assert
        assert_ne!(first[0].spans[1].style.fg, later[0].spans[1].style.fg);
        assert_eq!(first[0].spans[2].style.fg, Some(theme.text.secondary));
        assert_eq!(later[0].spans[2].style.fg, Some(theme.text.secondary));
    }

    #[test]
    fn static_reasoning_rail_is_painted_after_content() {
        // arrange
        let theme = Theme::default();
        let surface = reasoning_surface(&theme, "◆", None);
        let backend = TestBackend::new(20, 1);
        let mut terminal = Terminal::new(backend).expect("test terminal");

        // act
        terminal
            .draw(|frame| {
                render_transcript_surface(frame, &surface, Rect::new(0, 0, 20, 1), 0, 0, &theme);
            })
            .expect("render reasoning surface");

        // assert
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "┃");
    }
}

fn apply_tool_header_motion_color(
    line: &mut Line<'static>,
    surface: &TranscriptVisualEntry,
    absolute_row: usize,
    animation_phase: usize,
    theme: &Theme,
) {
    let Some(motion) = surface.tool_rail_motion else {
        return;
    };
    if surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning {
        if absolute_row == 0 {
            let color = tool_rail_motion_color(
                surface.surface,
                surface.rail_color,
                Some(motion),
                absolute_row,
                animation_phase,
            );
            if let Some(marker) = line
                .spans
                .iter_mut()
                .find(|span| !span.content.trim().is_empty())
            {
                marker.style = marker.style.fg(color);
            }
        }
        return;
    }
    let semantic_group_surface = surface.lines.first().is_some_and(|header| {
        let marker_index = header.spans.iter().position(|span| {
            span.content
                .trim_start()
                .starts_with(theme.live_shell.transcript_glyphs.group_marker)
        });
        marker_index.is_some_and(|marker_index| {
            header
                .spans
                .iter()
                .skip(marker_index + 1)
                .find(|span| !span.content.trim().is_empty())
                .is_some_and(|span| span.content.as_ref() != "Ran ")
        })
    });
    if semantic_group_surface && absolute_row != 0 {
        return;
    }
    let marker_glyphs = [
        theme.live_shell.transcript_glyphs.tool_marker,
        theme.live_shell.transcript_glyphs.thought_marker,
        theme.live_shell.transcript_glyphs.group_marker,
    ];
    let marker_index = line.spans.iter().position(|span| {
        marker_glyphs
            .iter()
            .any(|marker| span.content.trim_start().starts_with(marker))
    });
    let Some(marker_index) = marker_index else {
        return;
    };
    let color = tool_rail_motion_color(
        surface.surface,
        surface.rail_color,
        Some(motion),
        absolute_row,
        animation_phase,
    );
    if semantic_group_surface {
        line.spans[marker_index].style = line.spans[marker_index].style.fg(color);
    } else {
        for span in &mut line.spans {
            span.style = span.style.fg(color);
        }
    }
}

fn transcript_surface_rail_lines_for_motion(
    surface: &TranscriptVisualEntry,
    local_scroll: usize,
    visible_height: usize,
    animation_phase: usize,
) -> Vec<Line<'static>> {
    let row_scoped_rail = surface
        .lines
        .iter()
        .any(|line| line_has_tool_rail(line, surface.rail_glyph));
    (0..visible_height)
        .map(|local_row| {
            let absolute_row = local_scroll.saturating_add(local_row);
            let glyph = surface
                .lines
                .get(absolute_row)
                .filter(|line| !row_scoped_rail || line_has_tool_rail(line, surface.rail_glyph))
                .map_or(" ", |_| surface.rail_glyph);
            let color = tool_rail_motion_color(
                surface.surface,
                surface.rail_color,
                surface.tool_rail_motion,
                absolute_row,
                animation_phase,
            );
            Line::from(Span::styled(
                glyph,
                Style::default().fg(color).bg(surface.surface),
            ))
        })
        .collect()
}

pub(super) fn line_has_tool_rail(line: &Line<'_>, rail_glyph: &str) -> bool {
    !rail_glyph.is_empty()
        && line.spans.first().is_some_and(|span| {
            span.content
                .trim_start_matches(char::is_whitespace)
                .starts_with(rail_glyph)
        })
}

const TOOL_RAIL_WAVE_ROWS: usize = 32;
const TOOL_RAIL_ANGULAR_SPEED: f32 = 4.5;
const TOOL_RAIL_MIN_BRIGHTNESS: f32 = 0.28;
const TOOL_RAIL_BRIGHTNESS_RANGE: f32 = 0.72;

pub(super) fn wave_brightness(elapsed: Duration, row: usize, wave_rows: usize) -> f32 {
    let elapsed_secs = elapsed.as_secs_f32();
    let wave_rows = wave_rows.max(1);
    let row = u16::try_from(row % wave_rows).unwrap_or(0);
    let wave_rows = u16::try_from(wave_rows).unwrap_or(u16::MAX);
    let spatial_phase = f32::from(row) / f32::from(wave_rows) * std::f32::consts::TAU;
    (elapsed_secs.mul_add(TOOL_RAIL_ANGULAR_SPEED, spatial_phase))
        .sin()
        .powi(2)
}

pub(super) fn tool_rail_motion_color(
    surface: Color,
    accent: Color,
    motion: Option<ToolRailMotion>,
    row: usize,
    animation_phase: usize,
) -> Color {
    match motion {
        Some(ToolRailMotion::Running { .. }) => {
            let elapsed = motion_elapsed(motion, animation_phase);
            let brightness = wave_brightness(elapsed, row, TOOL_RAIL_WAVE_ROWS);
            blend_color(
                surface,
                accent,
                TOOL_RAIL_MIN_BRIGHTNESS + brightness * TOOL_RAIL_BRIGHTNESS_RANGE,
            )
        }
        Some(ToolRailMotion::FinishFlash { .. })
        | Some(ToolRailMotion::Waiting)
        | Some(ToolRailMotion::Queued)
        | Some(ToolRailMotion::Settled)
        | None => accent,
    }
}

fn motion_elapsed(motion: Option<ToolRailMotion>, animation_phase: usize) -> Duration {
    let (elapsed, sampled_phase) = match motion {
        Some(ToolRailMotion::Running {
            elapsed,
            sampled_phase,
        }) => (elapsed, sampled_phase),
        _ => return Duration::ZERO,
    };
    let phase_delta = animation_phase.saturating_sub(sampled_phase);
    elapsed.saturating_add(Duration::from_millis(
        u64::try_from(phase_delta)
            .unwrap_or(u64::MAX)
            .saturating_mul(crate::scheduling::active_animation_period_ms()),
    ))
}

pub(super) fn visible_surface_lines(
    surface: &TranscriptVisualEntry,
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

pub(super) fn render_transcript_surface_lines<Entry>(surfaces: &[Entry]) -> Vec<Line<'static>>
where
    Entry: Borrow<TranscriptVisualEntryDraft>,
{
    let mut lines = Vec::new();
    for entry in surfaces {
        let surface = entry.borrow();
        for _ in 0..surface.leading_gap_rows {
            lines.push(Line::default());
        }
        lines.extend(surface.lines.iter().cloned());
        for _ in 0..surface.trailing_gap_rows {
            lines.push(Line::default());
        }
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
        | TranscriptRenderSurfaceKind::AssistantTool
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SurfaceLinkRun {
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
}

#[derive(Debug, Clone)]
pub(super) struct WrappedSurfaceRow {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<SurfaceLinkRun>,
}

pub(super) fn wrap_surface_spans_with_links(
    spans: Vec<Span<'static>>,
    links: &[SurfaceLinkRun],
    width: usize,
) -> Vec<WrappedSurfaceRow> {
    let source = spans
        .iter()
        .flat_map(|span| split_graphemes(span.content.as_ref()))
        .scan(0usize, |cell, cluster| {
            let start_cell = *cell;
            *cell = cell.saturating_add(usize::from(cluster.display_width()));
            Some((cluster.as_str().to_string(), start_cell, *cell))
        })
        .collect::<Vec<_>>();
    let rows = wrap_surface_spans(spans, width);
    let mut source_index = 0usize;

    rows.into_iter()
        .map(|spans| {
            let mut projected = Vec::<SurfaceLinkRun>::new();
            let mut output_cell = 0usize;
            for cluster in spans
                .iter()
                .flat_map(|span| split_graphemes(span.content.as_ref()))
            {
                while source
                    .get(source_index)
                    .is_some_and(|(text, _, _)| text != cluster.as_str())
                {
                    source_index = source_index.saturating_add(1);
                }
                let Some((_, source_start, source_end)) = source.get(source_index) else {
                    break;
                };
                let cluster_width = usize::from(cluster.display_width());
                for link in links
                    .iter()
                    .filter(|link| link.start_cell < *source_end && link.end_cell > *source_start)
                {
                    let output_end = output_cell.saturating_add(cluster_width);
                    if let Some(previous) = projected.last_mut().filter(|previous| {
                        previous.destination == link.destination && previous.end_cell == output_cell
                    }) {
                        previous.end_cell = output_end;
                    } else {
                        projected.push(SurfaceLinkRun {
                            start_cell: output_cell,
                            end_cell: output_end,
                            destination: link.destination.clone(),
                        });
                    }
                }
                output_cell = output_cell.saturating_add(cluster_width);
                source_index = source_index.saturating_add(1);
            }
            WrappedSurfaceRow {
                spans,
                links: projected,
            }
        })
        .collect()
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
        }

        if simple_grapheme_boundaries(&token_text) {
            let mut chunk = String::new();
            let mut chunk_width = 0usize;
            for character in token_text.chars() {
                let character_width = usize::from(char_display_width(character));
                if !chunk.is_empty() && chunk_width.saturating_add(character_width) > width {
                    current.push(Span::styled(std::mem::take(&mut chunk), token.style));
                    rows.push(current);
                    current = Vec::new();
                    chunk_width = 0;
                }
                chunk.push(character);
                chunk_width = chunk_width.saturating_add(character_width);
            }
            current_width = chunk_width;
            current.push(Span::styled(chunk, token.style));
            continue;
        }

        let clusters = split_graphemes(&token_text);
        let mut chunk = String::new();
        let mut chunk_width = 0usize;
        for cluster in clusters {
            let cluster_width = usize::from(cluster.display_width());
            if !chunk.is_empty() && chunk_width.saturating_add(cluster_width) > width {
                current.push(Span::styled(std::mem::take(&mut chunk), token.style));
                rows.push(current);
                current = Vec::new();
                chunk_width = 0;
            }
            chunk.push_str(cluster.as_str());
            chunk_width = chunk_width.saturating_add(cluster_width);
        }
        current_width = chunk_width;
        current.push(Span::styled(chunk, token.style));
    }

    if !current.is_empty() {
        rows.push(current);
    }

    rows
}

fn simple_grapheme_boundaries(text: &str) -> bool {
    text.chars().all(|character| {
        char_display_width(character) > 0
            && u32::from(character) <= 0xFFFF
            && character != '\u{200D}'
            && !matches!(character, '\u{1F1E6}'..='\u{1F1FF}' | '\u{1F3FB}'..='\u{1F3FF}')
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    fn wrapped_text(text: &str, width: usize) -> Vec<String> {
        wrap_surface_spans(vec![Span::raw(text.to_string())], width)
            .into_iter()
            .map(|row| row.into_iter().map(|span| span.content).collect())
            .collect()
    }

    fn legacy_wrapped_text(text: &str, width: usize) -> Vec<String> {
        let mut rows = Vec::new();
        let mut chunk = String::new();
        let mut chunk_width = 0usize;
        for cluster in split_graphemes(text) {
            let cluster_width = usize::from(cluster.display_width());
            if !chunk.is_empty() && chunk_width.saturating_add(cluster_width) > width {
                rows.push(std::mem::take(&mut chunk));
                chunk_width = 0;
            }
            chunk.push_str(cluster.as_str());
            chunk_width = chunk_width.saturating_add(cluster_width);
        }
        if !chunk.is_empty() {
            rows.push(chunk);
        }
        rows
    }

    #[test]
    fn long_token_fast_path_preserves_unicode_and_complex_grapheme_boundaries() {
        // arrange
        // act
        let simple = "abc界···xyz";
        // assert
        assert!(simple_grapheme_boundaries(simple));
        assert_eq!(wrapped_text(simple, 4), legacy_wrapped_text(simple, 4));
        assert!(!simple_grapheme_boundaries("e\u{301}x"));
        assert!(!simple_grapheme_boundaries("👨\u{200D}💻x"));
        assert!(!simple_grapheme_boundaries("🇫🇮x"));
        assert_eq!(wrapped_text("e\u{301}x", 1), ["e\u{301}", "x"]);
        assert_eq!(wrapped_text("👨\u{200D}💻x", 2), ["👨\u{200D}💻", "x"]);
        assert_eq!(wrapped_text("🇫🇮x", 2), ["🇫🇮", "x"]);
    }
}
