#![allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
struct ComposerViewport {
    lines: Vec<String>,
    line_starts: Vec<usize>,
    cursor: Option<(usize, usize)>,
}
fn display_width(text: &str) -> usize {
    text.lines()
        .map(unicode_width::UnicodeWidthStr::width)
        .sum()
}
#[derive(Clone, Copy)]
struct ComposerVisualChar<'a> {
    index: usize,
    text: &'a str,
    width: usize,
}
type ComposerVisualLines = (Vec<(String, usize)>, Option<(usize, usize)>);
mod legacy {
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

        let (wrapped, cursor) = composer_visual_lines(text, width, cursor_char_index);

        let total_lines = wrapped.len();
        let visible_count = max_lines.min(total_lines).max(1);
        let anchor_row = cursor
            .map(|(row, _)| row)
            .unwrap_or(total_lines.saturating_sub(1));
        let start_row = anchor_row
            .saturating_add(1)
            .saturating_sub(visible_count)
            .min(total_lines.saturating_sub(visible_count));
        let end_row = start_row.saturating_add(visible_count).min(total_lines);

        ComposerViewport {
            lines: wrapped[start_row..end_row]
                .iter()
                .map(|(line, _)| line.clone())
                .collect(),
            line_starts: wrapped[start_row..end_row]
                .iter()
                .map(|(_, start)| *start)
                .collect(),
            cursor: cursor.and_then(|(row, column)| {
                (row >= start_row && row < end_row).then_some((row - start_row, column))
            }),
        }
    }

    fn composer_visual_lines(
        text: &str,
        width: usize,
        cursor_char_index: Option<usize>,
    ) -> ComposerVisualLines {
        let width = width.max(1);
        let mut lines = Vec::new();
        let mut cursor = None;
        let mut char_index = 0;
        let chars = text
            .graphemes(true)
            .map(|grapheme| {
                let index = char_index;
                char_index += grapheme.chars().count();
                ComposerVisualChar {
                    index,
                    text: grapheme,
                    width: display_width(grapheme).max(1),
                }
            })
            .collect::<Vec<_>>();

        let mut segment_start = 0usize;
        let mut fallback_start = 0usize;
        for position in 0..=chars.len() {
            let hard_break = position == chars.len() || chars[position].text == "\n";
            if !hard_break {
                continue;
            }

            wrap_composer_visual_segment(
                &chars[segment_start..position],
                fallback_start,
                width,
                cursor_char_index,
                &mut cursor,
                &mut lines,
            );

            if position < chars.len() {
                if cursor_char_index == Some(chars[position].index) {
                    let row = lines.len().saturating_sub(1);
                    let column = lines
                        .last()
                        .map(|(line, _)| display_width(line))
                        .unwrap_or(0);
                    cursor = Some((row, column));
                }
                fallback_start = chars[position].index + 1;
                segment_start = position + 1;
            }
        }

        if cursor_char_index == Some(text.chars().count()) {
            let row = lines.len().saturating_sub(1);
            let column = lines
                .last()
                .map(|(line, _)| display_width(line))
                .unwrap_or(0);
            cursor = Some((row, column));
        }

        (lines, cursor)
    }

    fn wrap_composer_visual_segment(
        chars: &[ComposerVisualChar],
        fallback_start: usize,
        width: usize,
        cursor_char_index: Option<usize>,
        cursor: &mut Option<(usize, usize)>,
        lines: &mut Vec<(String, usize)>,
    ) {
        if chars.is_empty() {
            emit_composer_visual_line(chars, fallback_start, cursor_char_index, cursor, lines);
            return;
        }

        let mut start = 0usize;
        while start < chars.len() {
            let fit_end = composer_fit_end(chars, start, width);
            if fit_end >= chars.len() {
                emit_composer_visual_line(
                    &chars[start..],
                    chars[start].index,
                    cursor_char_index,
                    cursor,
                    lines,
                );
                break;
            }

            if let Some(break_at) = chars[start..fit_end]
                .iter()
                .rposition(|visual_char| visual_char.text.chars().all(char::is_whitespace))
                .map(|offset| start + offset)
                .filter(|break_at| *break_at > start)
            {
                let end = break_at + 1;
                emit_composer_visual_line(
                    &chars[start..end],
                    chars[start].index,
                    cursor_char_index,
                    cursor,
                    lines,
                );
                start = end;
                continue;
            }

            if chars[fit_end].text.chars().all(char::is_whitespace) {
                emit_composer_visual_line(
                    &chars[start..fit_end],
                    chars[start].index,
                    cursor_char_index,
                    cursor,
                    lines,
                );
                if cursor_char_index == Some(chars[fit_end].index) {
                    let row = lines.len().saturating_sub(1);
                    let column = lines
                        .last()
                        .map(|(line, _)| display_width(line))
                        .unwrap_or(0);
                    *cursor = Some((row, column));
                }
                start = fit_end + 1;
                continue;
            }

            let end = fit_end.max(start + 1);
            emit_composer_visual_line(
                &chars[start..end],
                chars[start].index,
                cursor_char_index,
                cursor,
                lines,
            );
            start = end;
        }
    }

    fn composer_fit_end(chars: &[ComposerVisualChar], start: usize, width: usize) -> usize {
        let mut used = 0usize;
        for (position, visual_char) in chars.iter().enumerate().skip(start) {
            if position > start && used.saturating_add(visual_char.width) > width {
                return position;
            }
            used = used.saturating_add(visual_char.width);
        }
        chars.len()
    }

    fn emit_composer_visual_line(
        chars: &[ComposerVisualChar],
        fallback_start: usize,
        cursor_char_index: Option<usize>,
        cursor: &mut Option<(usize, usize)>,
        lines: &mut Vec<(String, usize)>,
    ) {
        let row = lines.len();
        let line_start = chars
            .first()
            .map(|visual_char| visual_char.index)
            .unwrap_or(fallback_start);
        if let Some(cursor_index) = cursor_char_index {
            if let Some(last) = chars.last() {
                let line_end = last.index + last.text.chars().count();
                if cursor_index >= line_start && cursor_index < line_end {
                    let column = chars
                        .iter()
                        .take_while(|visual_char| visual_char.index < cursor_index)
                        .map(|visual_char| visual_char.width)
                        .sum();
                    *cursor = Some((row, column));
                }
            } else if cursor_index == line_start {
                *cursor = Some((row, 0));
            }
        }

        lines.push((
            chars.iter().map(|visual_char| visual_char.text).collect(),
            line_start,
        ));
    }
}
mod text {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/text.rs"));
}
mod candidate {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/ui_composer/viewport.rs"
    ));
}
#[test]
fn recorded_composer_viewport_contract() {
    let pieces = [
        "a",
        " ",
        "\t",
        "\n",
        "\r\n",
        "界",
        "e\u{301}",
        "👩‍💻",
        "\u{301}",
        "\u{200b}",
    ];
    let mut texts = vec![String::new()];
    for a in pieces {
        for b in pieces {
            for c in pieces {
                texts.push(format!("{a}{b}{c}"));
            }
        }
    }
    texts.extend(
        [
            "ab \n",
            "ab \nc",
            "ab\r\nc",
            " abc defghijk",
            "one\n\ntwo\n",
            "  a  b  ",
            "🇫🇮🇬🇧👩‍👩‍👧‍👦",
        ]
        .map(str::to_owned),
    );
    let mut cases = 0;
    for text in texts {
        for width in [0, 1, 2, 3, 5, 10] {
            for max_lines in [0, 1, 2, 4] {
                for cursor in std::iter::once(None).chain((0..=text.chars().count() + 1).map(Some))
                {
                    assert_eq!(
                        candidate::composer_viewport(&text, width, max_lines, cursor),
                        legacy::composer_viewport(&text, width, max_lines, cursor),
                        "{text:?}, width {width}, rows {max_lines}, cursor {cursor:?}"
                    );
                    cases += 1;
                }
            }
        }
    }
    eprintln!("{cases} exact predecessor/candidate viewport comparisons");
}
