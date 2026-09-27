#[cfg(test)]
use std::borrow::Borrow;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use super::super::ui_chrome::display_width;
use super::super::ui_transcript::TranscriptRenderSurfaceKind;
#[cfg(test)]
use super::super::ui_transcript::TranscriptVisualEntryDraft;
use super::{wrap_preformatted_spans, wrap_surface_spans};

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 1;
// Grok Build HorizontalLayout::ACCENT (1) + LayoutConfig::block_pad_left (2).
pub(in crate::ui) const TRANSCRIPT_ENTRY_CONTENT_PREFIX: &str = "   ";
pub(in crate::ui) const TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH: u16 = 2;
pub(in crate::ui) const TRANSCRIPT_RAIL_GLYPH: &str = " ";

#[cfg(test)]
pub(in crate::ui) fn render_transcript_surface_lines<Entry>(
    surfaces: &[Entry],
) -> Vec<Line<'static>>
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

pub(in crate::ui) fn transcript_surface_content_width(width: u16, show_outer_rail: bool) -> u16 {
    if show_outer_rail {
        width.saturating_sub(TRANSCRIPT_SURFACE_RAIL_WIDTH).max(1)
    } else {
        width.max(1)
    }
}

pub(in crate::ui) fn transcript_surface_render_width(
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

pub(in crate::ui) fn append_prebuilt_nested_surface_lines(
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

pub(in crate::ui) fn append_prebuilt_surface_lines(
    lines: &mut Vec<Line<'static>>,
    indent: &str,
    surface: Color,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let prefix = surface_prefix(indent);
    let prefix_width = surface_prefix_width(indent);
    for line in prebuilt {
        // A tool can mix unbacked metadata with backed output rows. Preserve
        // that row's style through the shared surface without painting the rail.
        let row_surface = line.style.bg.unwrap_or(surface);
        let spans = line
            .spans
            .into_iter()
            .map(|span| Span::styled(span.content, line.style.patch(span.style)))
            .collect();
        lines.push(surface_line(
            prefix.clone(),
            prefix_width,
            spans,
            width,
            row_surface,
        ));
    }
}

pub(in crate::ui) fn append_surface_row(
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

pub(in crate::ui) fn append_user_surface_text_block(
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

pub(in crate::ui) fn append_user_surface_text_block_with_first_line_reserve(
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

pub(in crate::ui) fn append_user_surface_wrapped_line(
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

pub(in crate::ui) fn user_surface_line(
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

pub(in crate::ui) fn append_prefixed_wrapped_spans_line(
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

pub(in crate::ui) fn append_prebuilt_plain_lines(
    lines: &mut Vec<Line<'static>>,
    prefix: &str,
    prebuilt: Vec<Line<'static>>,
    width: u16,
) {
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);
    for line in prebuilt {
        for row in wrap_preformatted_spans(line.spans, content_width) {
            let mut spans = vec![Span::raw(prefix.to_string())];
            spans.extend(row);
            lines.push(Line::from(spans));
        }
    }
}

fn surface_prefix(indent: &str) -> Vec<Span<'static>> {
    if indent.is_empty() {
        Vec::new()
    } else {
        vec![Span::raw(indent.to_string())]
    }
}

pub(in crate::ui) fn surface_prefix_width(indent: &str) -> usize {
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

pub(in crate::ui) fn surface_span(
    text: impl Into<String>,
    style: Style,
    surface: Color,
) -> Span<'static> {
    Span::styled(text.into(), Style::default().bg(surface).patch(style))
}

pub(in crate::ui) fn append_nested_surface_row(
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

pub(in crate::ui) fn nested_surface_prefix_width(indent: &str) -> usize {
    display_width(indent) + display_width(TRANSCRIPT_RAIL_GLYPH) + 1
}
