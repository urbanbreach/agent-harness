use super::*;

pub(crate) fn composer_line_with_file_tags(
    line: &str,
    line_start: usize,
    tags: &[crate::app::FileMentionTag],
    base_style: Style,
    tag_style: Style,
) -> Line<'static> {
    if line.is_empty() {
        return Line::from(Span::styled(String::new(), base_style));
    }

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_style = None;
    for (offset, ch) in line.chars().enumerate() {
        let char_index = line_start + offset;
        let style = if tags
            .iter()
            .any(|tag| char_index >= tag.start && char_index < tag.end)
        {
            tag_style
        } else {
            base_style
        };
        if current_style == Some(style) {
            current.push(ch);
        } else {
            if !current.is_empty() {
                spans.push(Span::styled(
                    std::mem::take(&mut current),
                    current_style.unwrap_or_abort(),
                ));
            }
            current_style = Some(style);
            current.push(ch);
        }
    }
    if !current.is_empty() {
        spans.push(Span::styled(current, current_style.unwrap_or(base_style)));
    }
    Line::from(spans)
}
