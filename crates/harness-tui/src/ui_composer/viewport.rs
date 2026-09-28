use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn composer_viewport(
    text: &str,
    width: usize,
    max_lines: usize,
    cursor_char_index: Option<usize>,
) -> ComposerViewport {
    if max_lines == 0 {
        return ComposerViewport {
            lines: Vec::new(),
            line_starts: Vec::new(),
            cursor: None,
        };
    }

    let mut rows = Vec::new();
    let mut cursor = None;
    let mut byte_start = 0;
    let mut char_start = 0;
    // Only a standalone LF is a hard break. CRLF remains one whitespace grapheme.
    for byte_end in text
        .grapheme_indices(true)
        .filter_map(|(byte, grapheme)| (grapheme == "\n").then_some(byte))
        .chain(std::iter::once(text.len()))
    {
        let mut rest = &text[byte_start..byte_end];
        loop {
            let (end, next) = row_end(rest, width.max(1));
            let line = &rest[..end];
            let char_end = char_start + line.chars().count();
            if let Some(index) = cursor_char_index {
                if (char_start..char_end).contains(&index)
                    || (line.is_empty() && index == char_start)
                {
                    cursor = Some((rows.len(), cursor_column(line, index - char_start)));
                } else if index == char_end && next > end {
                    // A dropped overflow space belongs to the preceding row.
                    cursor = Some((rows.len(), display_width(line)));
                }
            }
            rows.push((line, char_start));
            char_start = char_end + rest[end..next].chars().count();
            rest = &rest[next..];
            if rest.is_empty() {
                break;
            }
        }
        // LF and EOF use the painted width, including zero-width graphemes.
        if cursor_char_index == Some(char_start) {
            cursor = rows
                .last()
                .map(|(line, _)| (rows.len() - 1, display_width(line)));
        }
        char_start += usize::from(byte_end < text.len());
        byte_start = byte_end.saturating_add(1);
    }

    let visible_count = max_lines.min(rows.len());
    let anchor = cursor.map_or(rows.len() - 1, |(row, _)| row);
    let start = anchor
        .saturating_add(1)
        .saturating_sub(visible_count)
        .min(rows.len() - visible_count);
    let end = start + visible_count;
    ComposerViewport {
        lines: rows[start..end]
            .iter()
            .map(|(line, _)| (*line).to_owned())
            .collect(),
        line_starts: rows[start..end].iter().map(|(_, index)| *index).collect(),
        cursor: cursor
            .and_then(|(row, column)| (start..end).contains(&row).then_some((row - start, column))),
    }
}

fn cursor_column(line: &str, index: usize) -> usize {
    let mut position = 0;
    let mut column = 0;
    for grapheme in line.graphemes(true) {
        if position >= index {
            break;
        }
        position += grapheme.chars().count();
        column += display_width(grapheme).max(1);
    }
    column
}

// Return the painted byte range and the next row's start, skipping only an
// overflowing whitespace grapheme when no earlier word boundary can be used.
fn row_end(text: &str, width: usize) -> (usize, usize) {
    let mut used = 0usize;
    let mut word_break = None;
    for (byte, grapheme) in text.grapheme_indices(true) {
        let whitespace = grapheme.chars().all(char::is_whitespace);
        let cells = display_width(grapheme).max(1);
        if byte > 0 && used.saturating_add(cells) > width {
            return match word_break {
                Some(end) => (end, end),
                None => (byte, byte + if whitespace { grapheme.len() } else { 0 }),
            };
        }
        used = used.saturating_add(cells);
        if whitespace && byte > 0 {
            word_break = Some(byte + grapheme.len());
        }
    }
    (text.len(), text.len())
}
