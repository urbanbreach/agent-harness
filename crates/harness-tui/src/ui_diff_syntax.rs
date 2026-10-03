use ratatui::{style::Style, text::Span};
use unicode_segmentation::UnicodeSegmentation;

use super::super::ui_chrome::{display_width, take_width_prefix};

#[derive(Debug, Clone)]
pub(crate) struct StyledTextChunk {
    pub(crate) text: String,
    pub(crate) style: Style,
}

pub(super) fn styled_chunks_to_spans(chunks: Vec<StyledTextChunk>) -> Vec<Span<'static>> {
    chunks
        .into_iter()
        .map(|chunk| Span::styled(chunk.text, chunk.style))
        .collect()
}

pub(super) fn wrap_styled_chunks(
    chunks: &[StyledTextChunk],
    max_width: usize,
) -> Vec<Vec<StyledTextChunk>> {
    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut text = String::new();
    let mut styles = Vec::new();
    let mut source_column = 0;
    for chunk in chunks {
        let start = text.len();
        for grapheme in chunk.text.graphemes(true) {
            if grapheme == "\t" {
                let spaces = 4 - source_column % 4;
                text.extend(std::iter::repeat_n(' ', spaces));
                source_column += spaces;
            } else {
                text.push_str(grapheme);
                source_column += display_width(grapheme);
            }
        }
        if text.len() > start {
            styles.push((start..text.len(), chunk.style));
        }
    }

    // Match Reference's whitespace-inclusive word wrapping before projecting styles:
    // syntax/intraline span boundaries must never become wrapping boundaries.
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    let mut used = 0;
    for word in text.split_inclusive(char::is_whitespace) {
        if used + display_width(word) > max_width && offset > start {
            ranges.push(start..offset);
            start = offset;
            used = 0;
        }
        let mut rest = word;
        while display_width(rest) > max_width {
            // Unlike the reference's over-width word, retain the whole source
            // on screen by splitting oversized tokens only at grapheme edges.
            let mut piece = take_width_prefix(rest, max_width);
            if piece.is_empty() {
                // A two-cell glyph in a one-cell viewport must make progress.
                piece = rest.graphemes(true).next().unwrap_or(rest);
            }
            offset += piece.len();
            ranges.push(start..offset);
            start = offset;
            rest = &rest[piece.len()..];
        }
        offset += rest.len();
        used += display_width(rest);
    }
    if offset > start || ranges.is_empty() {
        ranges.push(start..offset);
    }

    let mut style_index = 0;
    ranges
        .into_iter()
        .map(|range| {
            let mut row = Vec::new();
            let mut cursor = range.start;
            while cursor < range.end {
                let (source, style) = &styles[style_index];
                let end = source.end.min(range.end);
                row.push(StyledTextChunk {
                    text: text[cursor..end].to_string(),
                    style: *style,
                });
                cursor = end;
                if cursor == source.end {
                    style_index += 1;
                }
            }
            row
        })
        .collect()
}

pub(super) struct RecordedDiffHighlights {
    pub(super) before: Vec<ratatui::text::Line<'static>>,
    pub(super) after: Vec<ratatui::text::Line<'static>>,
}

pub(super) fn recorded_diff_highlights(
    file: &super::ui_diff_model::StructuredDiffFile,
    source: Option<&str>,
    theme: &crate::theme::Theme,
) -> RecordedDiffHighlights {
    use super::ui_diff_model::StructuredDiffDisplayRow;
    let mut before: Vec<String> = source.map_or_else(Vec::new, |source| {
        source.lines().map(str::to_string).collect()
    });
    if source.is_none() {
        for row in &file.rows {
            let item = match row {
                StructuredDiffDisplayRow::Context {
                    before_line: Some(line),
                    text,
                    ..
                } => Some((*line, text)),
                StructuredDiffDisplayRow::Changed {
                    before: Some(cell), ..
                } => cell.line_number.map(|line| (line, &cell.text)),
                _ => None,
            };
            if let Some((line, text)) = item.filter(|(line, _)| (1..=100_000).contains(line)) {
                before.resize(before.len().max(line), String::new());
                before[line - 1] = text.clone();
            }
        }
    }
    let mut after = Vec::new();
    let mut cursor = 0;
    for row in &file.rows {
        let (old_line, replacement) = match row {
            StructuredDiffDisplayRow::Context {
                before_line, text, ..
            } => (*before_line, Some(text)),
            StructuredDiffDisplayRow::Changed { before, after } => (
                before.as_ref().and_then(|cell| cell.line_number),
                after.as_ref().map(|cell| &cell.text),
            ),
            _ => continue,
        };
        if let Some(line) = old_line.filter(|line| *line > 0) {
            let start = (line - 1).min(before.len());
            if start > cursor {
                after.extend_from_slice(&before[cursor..start]);
            }
            cursor = line.min(before.len());
        }
        if let Some(text) = replacement {
            after.push(text.clone());
        }
    }
    after.extend_from_slice(&before[cursor..]);
    let language = file
        .after_path
        .as_deref()
        .or(file.before_path.as_deref())
        .or(Some(file.display_path.as_str()));
    let highlight = |lines: &[String]| {
        let body = lines.join("\n");
        super::super::ui_syntax_highlight::render_highlighted_code_block(
            language,
            &body,
            &body,
            "",
            theme.text.primary,
            theme,
        )
    };
    RecordedDiffHighlights {
        before: highlight(&before),
        after: highlight(&after),
    }
}

impl RecordedDiffHighlights {
    pub(super) fn before_line(
        &self,
        number: Option<usize>,
    ) -> Option<&ratatui::text::Line<'static>> {
        self.before.get(number?.checked_sub(1)?)
    }
    pub(super) fn after_line(
        &self,
        number: Option<usize>,
    ) -> Option<&ratatui::text::Line<'static>> {
        self.after.get(number?.checked_sub(1)?)
    }
}
