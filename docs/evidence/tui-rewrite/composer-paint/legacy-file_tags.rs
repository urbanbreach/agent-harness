use super::*;
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn composer_line_with_file_tags(
    line: &str,
    line_start: usize,
    tags: &[crate::app::FileMentionTag],
    base_style: Style,
    tag_style: Style,
    selection: Option<std::ops::Range<usize>>,
) -> Line<'static> {
    if line.is_empty() {
        return Line::from(Span::styled(String::new(), base_style));
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = None;
    let mut char_index = line_start;
    for grapheme in line.graphemes(true) {
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
        if current_style == Some(style) {
            current.push_str(grapheme);
        } else {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    current_style.unwrap_or_abort(),
                ));
            }
            current_style = Some(style);
            current.push_str(grapheme);
        }
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, current_style.unwrap_or(base_style)));
    }
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
