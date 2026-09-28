use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn composer_line_with_file_tags<'a>(
    line: &'a str,
    line_start: usize,
    tags: &[crate::app::FileMentionTag],
    base_style: Style,
    tag_style: Style,
    selection: Option<std::ops::Range<usize>>,
) -> Line<'a> {
    let mut spans = Vec::new();
    let mut start = 0;
    let mut current_style = base_style;
    let mut char_index = line_start;
    for (byte, grapheme) in line.grapheme_indices(true) {
        let end = char_index + grapheme.chars().count();
        let mut style = if tags
            .iter()
            .any(|tag| char_index >= tag.start && char_index < tag.end)
        {
            tag_style
        } else {
            base_style
        };
        if selection
            .as_ref()
            .is_some_and(|range| char_index < range.end && end > range.start)
        {
            style = style.add_modifier(Modifier::REVERSED);
        }
        char_index = end;
        if byte > 0 && current_style != style {
            spans.push(Span::styled(&line[start..byte], current_style));
            start = byte;
        }
        current_style = style;
    }
    spans.push(Span::styled(&line[start..], current_style));
    Line::from(spans)
}

pub(super) fn composer_selection(app: &AppState) -> Option<std::ops::Range<usize>> {
    if app.composer_disabled() || app.collapsed_paste_presentation().is_some() {
        return None;
    }
    let anchor = app.composer.selection_anchor?;
    let cursor = app.composer_render_cursor();
    Some(anchor.min(cursor)..anchor.max(cursor))
}
