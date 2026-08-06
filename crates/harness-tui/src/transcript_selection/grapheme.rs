use unicode_width::UnicodeWidthChar;

use super::selection_types::{CellPoint, Grapheme, GraphemeRange};

pub(crate) fn segment(text: &str) -> Vec<Grapheme> {
    let mut output = Vec::new();
    let mut start = None;
    let mut last = None;
    let mut regional_count = 0;
    for (byte, character) in text.char_indices() {
        let begins = start.is_none()
            || (!is_extend(character)
                && last != Some('\u{200d}')
                && !(is_regional(character) && regional_count == 1));
        if begins {
            if let Some(begin) = start {
                push(text, begin, byte, &mut output);
            }
            start = Some(byte);
            regional_count = 0;
        }
        if is_regional(character) {
            regional_count += 1;
        }
        last = Some(character);
    }
    if let Some(begin) = start {
        push(text, begin, text.len(), &mut output);
    }
    output
}

fn push(text: &str, start: usize, end: usize, output: &mut Vec<Grapheme>) {
    let value = &text[start..end];
    let width = display_width(value);
    let cell_start = output
        .last()
        .map_or(0, |last: &Grapheme| last.range.cell_range.end);
    output.push(Grapheme {
        text: value.to_string(),
        range: GraphemeRange {
            byte_range: start..end,
            cell_range: cell_start..cell_start + width,
        },
        end: CellPoint::new(0, cell_start + width.saturating_sub(1)),
    });
}

pub(crate) fn display_width(value: &str) -> usize {
    let mut width = value
        .chars()
        .filter_map(UnicodeWidthChar::width)
        .max()
        .unwrap_or(1);
    if value
        .chars()
        .filter(|character| is_regional(*character))
        .count()
        == 2
    {
        width = 2;
    }
    width.max(1)
}

fn is_extend(character: char) -> bool {
    character == '\u{200d}'
        || character == '\u{fe0e}'
        || character == '\u{fe0f}'
        || matches!(
            character,
            '\u{0300}'..='\u{036f}'
                | '\u{1ab0}'..='\u{1aff}'
                | '\u{1dc0}'..='\u{1dff}'
                | '\u{20d0}'..='\u{20ff}'
                | '\u{fe20}'..='\u{fe2f}'
                | '\u{1f3fb}'..='\u{1f3ff}'
        )
}

fn is_regional(character: char) -> bool {
    matches!(character, '\u{1f1e6}'..='\u{1f1ff}')
}
