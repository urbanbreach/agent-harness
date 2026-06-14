use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Paragraph},
    Frame,
};

use crate::theme::Theme;

use super::ui_chrome::{display_width, panel_style, take_width_prefix};
use super::ui_transcript::{TranscriptRenderSurface, TranscriptRenderSurfaceKind};
use super::ui_transcript_layout::MeasuredTranscriptSurface;
use super::ui_transcript_style::decorate_transcript_spinner_line;

const TRANSCRIPT_SURFACE_RAIL_WIDTH: u16 = 1;
const TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH: u16 = 2;
pub(super) const TRANSCRIPT_RAIL_GLYPH: &str = "┃";

pub(super) fn transcript_surface_leading_gap(
    previous: Option<TranscriptRenderSurfaceKind>,
    current: TranscriptRenderSurfaceKind,
) -> usize {
    match previous {
        Some(previous)
            if transcript_surface_is_assistant_tool_like(previous)
                && transcript_surface_is_assistant_tool_like(current) =>
        {
            0
        }
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantBody
                && transcript_surface_is_assistant_tool_like(current) =>
        {
            0
        }
        Some(previous)
            if previous == TranscriptRenderSurfaceKind::AssistantReasoning
                && current == TranscriptRenderSurfaceKind::AssistantBody =>
        {
            0
        }
        Some(previous)
            if transcript_surface_is_assistant_tool_like(previous)
                && current == TranscriptRenderSurfaceKind::AssistantReasoning =>
        {
            0
        }
        Some(previous)
            if matches!(
                previous,
                TranscriptRenderSurfaceKind::AssistantBody
                    | TranscriptRenderSurfaceKind::AssistantReasoning
                    | TranscriptRenderSurfaceKind::AssistantTool
                    | TranscriptRenderSurfaceKind::AssistantCommandTool
                    | TranscriptRenderSurfaceKind::AssistantError
            ) && current == TranscriptRenderSurfaceKind::AssistantFooter =>
        {
            1
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
    theme: &Theme,
    animation_phase: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    frame.render_widget(
        Block::default().style(Style::default().bg(surface.surface)),
        area,
    );

    let rail_width = if surface.show_outer_rail {
        area.width.min(TRANSCRIPT_SURFACE_RAIL_WIDTH)
    } else {
        0
    };
    if rail_width > 0 {
        let rail_rect = Rect::new(area.x, area.y, rail_width, area.height);
        frame.render_widget(
            Paragraph::new(transcript_surface_rail_lines(
                usize::from(area.height),
                surface.rail_color,
                surface.surface,
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
    let visible_lines = visible_surface_lines(
        surface,
        local_scroll,
        usize::from(content_rect.height),
        animation_phase,
    );
    let paragraph = Paragraph::new(Text::from(visible_lines))
        .style(panel_style(surface.surface, theme.text.primary));
    frame.render_widget(paragraph, content_rect);
}

pub(super) fn visible_surface_lines(
    surface: &MeasuredTranscriptSurface,
    local_scroll: usize,
    visible_height: usize,
    animation_phase: usize,
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
        .map(|line| decorate_transcript_spinner_line(line, animation_phase))
        .collect()
}

pub(super) fn render_transcript_surface_lines(
    surfaces: &[TranscriptRenderSurface],
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for surface in surfaces {
        if surface.show_outer_rail {
            lines.extend(surface.lines.iter().cloned().map(|line| {
                prepend_transcript_surface_rail(line, surface.rail_color, surface.surface)
            }));
        } else {
            lines.extend(surface.lines.iter().cloned());
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
        TranscriptRenderSurfaceKind::User | TranscriptRenderSurfaceKind::AssistantCommandTool => {
            width
                .saturating_sub(TRANSCRIPT_SURFACE_TRAILING_GAP_WIDTH)
                .max(1)
        }
        TranscriptRenderSurfaceKind::AssistantFooter
        | TranscriptRenderSurfaceKind::AssistantReasoning
        | TranscriptRenderSurfaceKind::AssistantBody
        | TranscriptRenderSurfaceKind::AssistantTool
        | TranscriptRenderSurfaceKind::AssistantError => width.max(1),
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
        lines.push(nested_surface_line(
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
    let base_style = Style::default().fg(color);
    for line in text.lines() {
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
        );
    }

    if text.is_empty() {
        append_user_surface_wrapped_line(lines, Vec::new(), prefix, base_style, width, surface);
    }
}

fn append_user_surface_wrapped_line(
    lines: &mut Vec<Line<'static>>,
    content_spans: Vec<Span<'static>>,
    prefix: &str,
    prefix_style: Style,
    width: u16,
    surface: Color,
) {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    if content_spans.is_empty() {
        lines.push(user_surface_line(prefix, Vec::new(), prefix_style, surface));
        return;
    }

    for row in wrap_surface_spans(content_spans, content_width) {
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
    content_spans: Vec<Span<'static>>,
    width: u16,
) {
    let prefix = nested_surface_prefix(indent, rail_color, surface);
    let prefix_width = nested_surface_prefix_width(indent);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wrapped_rows = wrap_surface_spans(content_spans, content_width);

    if wrapped_rows.is_empty() {
        lines.push(nested_surface_line(
            prefix,
            prefix_width,
            Vec::new(),
            width,
            surface,
        ));
        return;
    }

    for row in wrapped_rows {
        lines.push(nested_surface_line(
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
    spans.push(surface_span("  ", Style::default(), surface));
    spans
}

pub(super) fn nested_surface_prefix_width(indent: &str) -> usize {
    display_width(indent) + display_width(TRANSCRIPT_RAIL_GLYPH) + 2
}

fn nested_surface_line(
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
    rail_color: Color,
    surface: Color,
) -> Text<'static> {
    Text::from(
        (0..height)
            .map(|_| {
                Line::from(Span::styled(
                    TRANSCRIPT_RAIL_GLYPH,
                    Style::default().fg(rail_color).bg(surface),
                ))
            })
            .collect::<Vec<_>>(),
    )
}

fn prepend_transcript_surface_rail(
    line: Line<'static>,
    rail_color: Color,
    surface: Color,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        TRANSCRIPT_RAIL_GLYPH,
        Style::default().fg(rail_color).bg(surface),
    )];
    spans.extend(line.spans);
    Line::from(spans)
}
