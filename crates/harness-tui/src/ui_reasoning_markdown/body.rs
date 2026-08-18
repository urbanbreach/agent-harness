use std::mem;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::ui_reasoning_markdown::{parse_reasoning_inline_spans, reasoning_markdown_colors};
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ReasoningSelectionMetadata {
    pub(super) continues_previous: bool,
    pub(super) copy_offset: usize,
}

pub(super) fn append_reasoning_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &str,
    theme: &Theme,
    surface: Color,
    prefix: &str,
    width: u16,
) -> Vec<ReasoningSelectionMetadata> {
    let colors = reasoning_markdown_colors(theme, surface);
    let base_style = Style::default().fg(colors.base);
    let mut paragraph = Vec::new();
    let mut selection_rows = Vec::new();
    let flush_paragraph =
        |lines: &mut Vec<Line<'static>>,
         paragraph: &mut Vec<Span<'static>>,
         selection_rows: &mut Vec<ReasoningSelectionMetadata>| {
            if !paragraph.is_empty() {
                let start = lines.len();
                append_prefixed_wrapped_spans_line(
                    lines,
                    prefix,
                    base_style,
                    mem::take(paragraph),
                    width,
                );
                selection_rows.extend(selection_metadata(
                    lines.len().saturating_sub(start),
                    display_width(prefix),
                ));
            }
        };

    for row in body.lines() {
        if row.is_empty() {
            flush_paragraph(lines, &mut paragraph, &mut selection_rows);
            let start = lines.len();
            append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
            selection_rows.extend(selection_metadata(
                lines.len().saturating_sub(start),
                display_width(prefix),
            ));
            continue;
        }

        let trimmed = row.trim_start();
        let indent = &row[..row.len() - trimmed.len()];

        if let Some(heading_text) = markdown_heading_text(trimmed) {
            flush_paragraph(lines, &mut paragraph, &mut selection_rows);
            let heading_style = Style::default()
                .fg(colors.heading)
                .add_modifier(Modifier::BOLD);
            let start = lines.len();
            append_prefixed_wrapped_spans_line(
                lines,
                &format!("{prefix}{indent}"),
                heading_style,
                parse_reasoning_inline_spans(heading_text, &colors),
                width,
            );
            selection_rows.extend(selection_metadata(
                lines.len().saturating_sub(start),
                display_width(prefix),
            ));
            continue;
        }

        if markdown_rule(trimmed) {
            flush_paragraph(lines, &mut paragraph, &mut selection_rows);
            let content_width = usize::from(width).saturating_sub(prefix.len()).max(1);
            let start = lines.len();
            append_prefixed_wrapped_spans_line(
                lines,
                prefix,
                base_style,
                vec![Span::styled(
                    "─".repeat(content_width),
                    Style::default().fg(colors.rule),
                )],
                width,
            );
            selection_rows.extend(selection_metadata(
                lines.len().saturating_sub(start),
                display_width(prefix),
            ));
            continue;
        }

        if let Some(text) = trimmed.strip_prefix("> ") {
            flush_paragraph(lines, &mut paragraph, &mut selection_rows);
            let quote_style = Style::default()
                .fg(colors.block_quote)
                .add_modifier(Modifier::ITALIC);
            let quote_prefix = format!("{prefix}{indent}▍ ");
            let start = lines.len();
            append_prefixed_wrapped_spans_line(
                lines,
                &quote_prefix,
                quote_style,
                parse_reasoning_inline_spans(text, &colors),
                width,
            );
            selection_rows.extend(selection_metadata(
                lines.len().saturating_sub(start),
                display_width(&quote_prefix),
            ));
            continue;
        }

        if let Some((marker, text, is_enum)) = parse_list_prefix(trimmed) {
            flush_paragraph(lines, &mut paragraph, &mut selection_rows);
            let marker_color = if is_enum {
                colors.list_enum
            } else {
                colors.list_marker
            };
            let marker_style = Style::default()
                .fg(marker_color)
                .add_modifier(Modifier::BOLD);
            let start = lines.len();
            append_prefixed_wrapped_spans_line(
                lines,
                &format!("{prefix}{indent}{marker}"),
                marker_style,
                parse_reasoning_inline_spans(text, &colors),
                width,
            );
            selection_rows.extend(selection_metadata(
                lines.len().saturating_sub(start),
                display_width(prefix),
            ));
            continue;
        }

        if !paragraph.is_empty() {
            paragraph.push(Span::styled(" ", base_style));
        }
        extend_coalesced(
            &mut paragraph,
            parse_reasoning_inline_spans(trimmed, &colors),
        );
    }

    flush_paragraph(lines, &mut paragraph, &mut selection_rows);
    selection_rows
}

fn selection_metadata(
    row_count: usize,
    copy_offset: usize,
) -> impl Iterator<Item = ReasoningSelectionMetadata> {
    (0..row_count).map(move |index| ReasoningSelectionMetadata {
        continues_previous: index > 0,
        copy_offset,
    })
}

fn extend_coalesced(target: &mut Vec<Span<'static>>, spans: Vec<Span<'static>>) {
    for span in spans {
        if let Some(previous) = target.last_mut() {
            if previous.style == span.style {
                previous.content.to_mut().push_str(span.content.as_ref());
                continue;
            }
        }
        target.push(span);
    }
}

fn parse_list_prefix(line: &str) -> Option<(String, &str, bool)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some(("• ".to_string(), text, false));
        }
    }
    let digits = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(text) = line[digits..].strip_prefix(". ") {
            return Some((format!("{}. ", &line[..digits]), text, true));
        }
    }
    None
}
