use std::ops::Range;

use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;

use super::ui_chrome::display_width;

pub(super) fn clip(mut spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let mut remaining = width;
    for span in &mut spans {
        if display_width(&span.content) > remaining {
            span.content = super::ui_chrome::take_width_prefix(&span.content, remaining)
                .to_string()
                .into();
        }
        remaining = remaining.saturating_sub(display_width(&span.content));
    }
    spans
}

/// The reference uses Unicode line breaks and greedy wrapping for tool prose.
/// Keep styled spans and reject a library break that would split a grapheme.
pub(super) fn words(spans: Vec<Span<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let text = spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect::<String>();
    let boundaries = text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let options =
        textwrap::Options::new(width.max(1)).wrap_algorithm(textwrap::WrapAlgorithm::FirstFit);
    let mut cursor = 0;
    let rows = textwrap::wrap(&text, options)
        .iter()
        .map(|row| {
            let start = cursor + text.get(cursor..)?.find(row.as_ref())?;
            let end = start + row.len();
            if boundaries.binary_search(&start).is_err() || boundaries.binary_search(&end).is_err()
            {
                return None;
            }
            cursor = end;
            Some(slice_spans(&spans, start..end))
        })
        .collect::<Option<Vec<_>>>();
    rows.unwrap_or_else(|| super::ui_transcript_surface::wrap_preformatted_spans(spans, width))
}

/// Shell display wraps after operators and between arguments. Quoted arguments
/// and heredoc payload stay intact; the terminal clips overlong rows.
pub(super) fn shell(lines: Vec<Line<'static>>, width: usize) -> Vec<Vec<Span<'static>>> {
    let text = lines
        .iter()
        .map(Line::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let syntax = shell_syntax(&text).unwrap_or_default();
    let mut offset = 0;
    let mut rows = Vec::new();
    for line in lines {
        let text = line.to_string();
        let payload = syntax
            .payloads
            .iter()
            .any(|range| range.start <= offset && offset + text.len() <= range.end);
        let breaks = syntax
            .operators
            .iter()
            .copied()
            .filter(|end| *end > offset && *end < offset + text.len())
            .map(|end| end - offset)
            .collect::<Vec<_>>();
        let ranges: Vec<Range<usize>> = if payload || text.is_empty() {
            std::iter::once(0..text.len()).collect()
        } else {
            pack_ranges(&text, breaks, width)
                .into_iter()
                .flat_map(|range| {
                    let part = &text[range.clone()];
                    pack_ranges(part, quote_breaks(part), width)
                        .into_iter()
                        .map(move |inner| (range.start + inner.start)..(range.start + inner.end))
                })
                .collect()
        };
        rows.extend(
            ranges
                .into_iter()
                .map(|range| slice_spans(&line.spans, range)),
        );
        offset += text.len() + 1;
    }
    rows
}

#[derive(Default)]
struct ShellSyntax {
    operators: Vec<usize>,
    payloads: Vec<Range<usize>>,
}

fn shell_syntax(text: &str) -> Option<ShellSyntax> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_bash::LANGUAGE.into())
        .ok()?;
    let tree = parser.parse(text, None)?;
    if tree.root_node().has_error() {
        return None;
    }
    let mut syntax = ShellSyntax::default();
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        match node.kind() {
            "heredoc_body" | "simple_heredoc_body" | "heredoc_content" => {
                syntax.payloads.push(node.byte_range());
                continue;
            }
            "heredoc_end" | "string" | "raw_string" | "string_content" | "ansi_c_string"
            | "translated_string" | "comment" => continue,
            "&&" | "||" | "|" | ";" => syntax.operators.push(node.end_byte()),
            _ => {}
        }
        stack.extend(node.children(&mut node.walk()));
    }
    syntax.operators.sort_unstable();
    syntax.operators.dedup();
    Some(syntax)
}

fn quote_breaks(text: &str) -> Vec<usize> {
    let mut quote = None;
    let mut escaped = false;
    let mut breaks = Vec::new();
    let mut whitespace = false;
    for (index, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if quote == Some('"') && ch == '\\' {
            escaped = true;
        } else if quote == Some(ch) {
            quote = None;
        } else if quote.is_none() && matches!(ch, '\'' | '"') {
            quote = Some(ch);
        } else if quote.is_none() && ch.is_ascii_whitespace() && !whitespace && index > 0 {
            breaks.push(index);
        }
        whitespace = ch.is_ascii_whitespace();
    }
    breaks
}

fn pack_ranges(text: &str, breaks: Vec<usize>, width: usize) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut last = 0;
    for end in breaks.into_iter().chain(std::iter::once(text.len())) {
        if display_width(text[start..end].trim_end()) > width && last > start {
            ranges.push(start..text[..last].trim_end().len());
            start = text.len()
                - text[last..]
                    .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
                    .len();
        }
        if display_width(text[start..end].trim_end()) > width {
            ranges.push(start..text[..end].trim_end().len());
            start = text.len()
                - text[end..]
                    .trim_start_matches(|ch: char| ch.is_ascii_whitespace())
                    .len();
        }
        last = end.max(start);
    }
    if start < text.len() {
        ranges.push(start..text.trim_end().len());
    }
    ranges
}

fn slice_spans(spans: &[Span<'static>], range: Range<usize>) -> Vec<Span<'static>> {
    let mut offset = 0;
    spans
        .iter()
        .filter_map(|span| {
            let start = range.start.saturating_sub(offset).min(span.content.len());
            let end = range.end.saturating_sub(offset).min(span.content.len());
            offset += span.content.len();
            (start < end)
                .then(|| span.content.get(start..end))
                .flatten()
                .map(|text| Span::styled(text.to_string(), span.style))
        })
        .collect()
}
