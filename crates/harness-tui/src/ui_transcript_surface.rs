use std::time::Duration;

use ratatui::{
    buffer::{Buffer, CellWidth},
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::theme::Theme;

use super::ui_transcript::{ToolRailMotion, TranscriptRenderSurfaceKind};
use super::ui_transcript_layout::TranscriptVisualEntry;
use super::ui_transcript_style::{
    blend_color, glyph_routed_streaming_spinner_frame, pending_diamond_color,
};

#[path = "ui_transcript_surface/rows.rs"]
mod rows;
pub(super) use rows::*;
#[path = "ui_transcript_surface/wrapping.rs"]
mod wrapping;
pub(super) use wrapping::{
    expand_preformatted_tabs, wrap_preformatted_spans, wrap_surface_spans,
    wrap_surface_spans_with_links, SurfaceLinkRun, WrappedSurfaceRow,
};

pub(super) fn render_transcript_surface(
    frame: &mut Frame,
    surface: &TranscriptVisualEntry,
    area: Rect,
    local_scroll: usize,
    animation_phase: usize,
    theme: &Theme,
    child_view: bool,
) {
    let buffer = frame.buffer_mut();
    let area = area.intersection(buffer.area);
    if area.is_empty() {
        return;
    }
    buffer.set_style(
        area,
        Style::default().bg(surface.surface).fg(if child_view {
            Color::Reset
        } else {
            theme.text.primary
        }),
    );
    let spinner = glyph_routed_streaming_spinner_frame(theme, animation_phase, true);
    let group = surface.tool_rail_motion.is_some()
        && surface.lines.first().is_some_and(|header| {
            header
                .spans
                .iter()
                .position(|span| {
                    span.content
                        .trim_start()
                        .starts_with(theme.live_shell.transcript_glyphs.group_marker)
                })
                .and_then(|index| {
                    header.spans[index + 1..]
                        .iter()
                        .find(|span| !span.content.trim().is_empty())
                })
                .is_some_and(|span| span.content != "Ran ")
        });
    for (y, line) in surface
        .lines
        .iter()
        .skip(local_scroll)
        .take(usize::from(area.height))
        .enumerate()
    {
        let row = local_scroll.saturating_add(y);
        let timestamp = surface
            .interaction_rows
            .as_ref()
            .and_then(|rows| rows.get(row))
            .and_then(Option::as_ref)
            .is_some_and(|row| {
                matches!(
                    row.target,
                    super::ui_transcript_interaction::TranscriptMouseTarget::UserTimestamp { .. }
                )
            });
        let line = child_line_style(line, child_view, timestamp, theme);
        let tool = tool_marker(&line, surface, row, group, spinner, theme).map(|index| {
            (
                index,
                tool_rail_motion_color(
                    surface.surface,
                    surface.rail_color,
                    surface.tool_rail_motion,
                    row,
                    animation_phase,
                ),
            )
        });
        let pending = matches!(
            surface.kind,
            TranscriptRenderSurfaceKind::User | TranscriptRenderSurfaceKind::AssistantFooter
        )
        .then(|| {
            marker_index(
                &line,
                spinner,
                &[
                    theme.live_shell.glyphs.pending_permission,
                    theme.live_shell.transcript_glyphs.tool_marker,
                    theme.live_shell.transcript_glyphs.thought_marker,
                    theme.live_shell.transcript_glyphs.group_marker,
                ],
            )
        })
        .flatten()
        .map(|index| (index, pending_diamond_color(theme, animation_phase)));
        paint_line(
            buffer,
            &line,
            Rect::new(
                area.x,
                area.y + u16::try_from(y).unwrap_or(0),
                area.width,
                1,
            ),
            [pending, tool],
            spinner,
        );
    }

    if surface.show_outer_rail
        || (surface.tool_rail_motion.is_some()
            && surface
                .lines
                .iter()
                .any(|line| line_has_tool_rail(line, surface.rail_glyph)))
    {
        for y in 0..area.height {
            paint_rail(
                buffer,
                surface,
                (area.x, area.y + y),
                local_scroll.saturating_add(usize::from(y)),
                animation_phase,
            );
        }
    }
}

fn child_line_style<'a>(
    line: &'a Line<'static>,
    child_view: bool,
    timestamp: bool,
    theme: &Theme,
) -> std::borrow::Cow<'a, Line<'static>> {
    if !child_view {
        return std::borrow::Cow::Borrowed(line);
    }
    let mut line = line.clone();
    if line.spans.iter().all(|span| span.content.trim().is_empty()) {
        for span in &mut line.spans {
            span.style = span.style.fg(Color::Reset);
        }
        return std::borrow::Cow::Owned(line);
    }
    trim_child_line_end(&mut line);
    if timestamp && let Some(clock) = line.spans.last_mut() {
        clock.style = clock.style.fg(theme.text.secondary);
    }
    if let Some(first) = line.spans.first_mut()
        && let Some(content) = first.content.strip_prefix(TRANSCRIPT_ENTRY_CONTENT_PREFIX)
    {
        first.content = content.to_owned().into();
        line.spans.insert(
            0,
            Span::styled(
                TRANSCRIPT_ENTRY_CONTENT_PREFIX,
                Style::default().fg(Color::Reset),
            ),
        );
    }
    std::borrow::Cow::Owned(line)
}

fn trim_child_line_end(line: &mut Line<'static>) {
    while let Some(span) = line.spans.last_mut() {
        if span.style.bg.is_some() {
            break;
        }
        let trimmed = span.content.trim_end_matches(' ');
        if trimmed.is_empty() {
            line.spans.pop();
        } else {
            span.content = trimmed.to_owned().into();
            break;
        }
    }
}

fn paint_content<'a>(span: &'a Span<'_>, spinner: &'a str) -> &'a str {
    match span.content.as_ref() {
        "⠋" | "⠙" | "⠹" | "⠸" | "⠼" | "⠴" | "⠦" | "⠧" => spinner,
        content => content,
    }
}

fn marker_index(line: &Line<'_>, spinner: &str, markers: &[&str]) -> Option<usize> {
    line.spans
        .iter()
        .position(|span| markers.contains(&paint_content(span, spinner).trim()))
}

fn tool_marker(
    line: &Line<'_>,
    surface: &TranscriptVisualEntry,
    row: usize,
    group: bool,
    spinner: &str,
    theme: &Theme,
) -> Option<usize> {
    if matches!(
        surface.tool_rail_motion,
        None | Some(ToolRailMotion::FinishFlash { .. })
    ) {
        return None;
    }
    if surface.kind == TranscriptRenderSurfaceKind::AssistantReasoning {
        return (row == 0)
            .then(|| {
                marker_index(
                    line,
                    spinner,
                    &[theme.live_shell.transcript_glyphs.tool_marker],
                )
            })
            .flatten();
    }
    if group && row != 0 {
        return None;
    }
    marker_index(
        line,
        spinner,
        &[
            theme.live_shell.glyphs.running,
            theme.live_shell.transcript_glyphs.tool_marker,
            theme.live_shell.transcript_glyphs.thought_marker,
            theme.live_shell.transcript_glyphs.group_marker,
        ],
    )
}

fn paint_line(
    buffer: &mut Buffer,
    line: &Line<'_>,
    area: Rect,
    markers: [Option<(usize, Color)>; 2],
    spinner: &str,
) {
    let width = usize::from(area.width);
    let clipped = || {
        let mut used = 0;
        line.spans.iter().enumerate().flat_map(|(index, span)| {
            let mut style = line.style.patch(span.style);
            if let Some((_, color)) = markers.iter().flatten().find(|(marker, _)| *marker == index) {
                style = style.fg(*color);
            }
            paint_content(span, spinner).graphemes(true)
                .filter(|symbol| !symbol.contains(char::is_control))
                .map(move |symbol| (symbol, style, usize::from(symbol.cell_width())))
        })
        // Paragraph skips oversized graphemes, then truncates at the first overflow.
        .filter(|(_, _, cells)| *cells <= width)
        .map_while(move |item| {
            used += item.2;
            (used <= width).then_some(item)
        })
    };
    let mut x = area.x;
    if let Some(alignment @ (Alignment::Center | Alignment::Right)) = line.alignment {
        let used = clipped().map(|(_, _, cells)| cells).sum::<usize>();
        let offset = match alignment {
            Alignment::Center => width / 2 - used / 2,
            _ => width - used,
        };
        x += u16::try_from(offset).unwrap_or(0);
    }
    for (symbol, style, cells) in clipped().filter(|(_, _, cells)| *cells > 0) {
        buffer[(x, area.y)].set_symbol(symbol).set_style(style);
        x += u16::try_from(cells).unwrap_or(0);
    }
}

fn paint_rail(
    buffer: &mut Buffer,
    surface: &TranscriptVisualEntry,
    position: (u16, u16),
    row: usize,
    phase: usize,
) {
    let line = surface.lines.get(row);
    let has_rail = line.is_some_and(|line| line_has_tool_rail(line, surface.rail_glyph));
    let glyph = if line.is_some() && (surface.show_outer_rail || has_rail) {
        surface.rail_glyph
    } else {
        " "
    };
    let tool = matches!(
        surface.kind,
        TranscriptRenderSurfaceKind::AssistantTool
            | TranscriptRenderSurfaceKind::AssistantCommandTool
    ) && !surface.show_outer_rail;
    let color = if tool {
        line.filter(|_| has_rail)
            .and_then(|line| line.spans.first())
            .and_then(|span| span.style.fg)
            .unwrap_or(surface.rail_color)
    } else {
        tool_rail_motion_color(
            surface.surface,
            surface.rail_color,
            surface.tool_rail_motion,
            row,
            phase,
        )
    };
    if let Some(symbol) = glyph
        .graphemes(true)
        .find(|symbol| !symbol.contains(char::is_control) && symbol.cell_width() == 1)
    {
        buffer[position]
            .set_symbol(symbol)
            .set_style(Style::default().fg(color).bg(surface.surface));
    }
}

pub(super) fn line_has_tool_rail(line: &Line<'_>, rail_glyph: &str) -> bool {
    !rail_glyph.is_empty()
        && line.spans.first().is_some_and(|span| {
            span.content
                .trim_start_matches(char::is_whitespace)
                .starts_with(rail_glyph)
        })
}

pub(super) fn wave_brightness(elapsed: Duration, row: usize, wave_rows: usize) -> f32 {
    let tick = elapsed.as_secs_f32() / 0.033;
    let wave_rows = wave_rows.max(1);
    let row = u16::try_from(row % wave_rows).unwrap_or(0);
    let wave_rows = u16::try_from(wave_rows).unwrap_or(u16::MAX);
    let spatial_phase = f32::from(row) / f32::from(wave_rows) * std::f32::consts::TAU;
    let sine = (tick * 0.15 + spatial_phase).sin();
    sine * sine
}

pub(super) fn tool_rail_motion_color(
    surface: Color,
    accent: Color,
    motion: Option<ToolRailMotion>,
    row: usize,
    animation_phase: usize,
) -> Color {
    let Some(ToolRailMotion::Running {
        elapsed,
        sampled_phase,
    }) = motion
    else {
        return accent;
    };
    let phase_delta = animation_phase.saturating_sub(sampled_phase);
    let elapsed = elapsed.saturating_add(Duration::from_millis(
        u64::try_from(phase_delta)
            .unwrap_or(u64::MAX)
            .saturating_mul(crate::scheduling::active_animation_period_ms()),
    ));
    blend_color(surface, accent, wave_brightness(elapsed, row, 32))
}

#[cfg(test)]
#[path = "ui_transcript_surface/paint_tests.rs"]
mod paint_tests;
