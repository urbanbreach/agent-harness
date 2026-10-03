use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::theme::Theme;

pub(super) fn raw_markdown_lines(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    text.split('\n')
        .map(|line| {
            Line::from(Span::styled(
                line.to_owned(),
                Style::default().fg(theme.markdown.text),
            ))
        })
        .collect()
}

pub(super) fn append_raw_markdown(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    for line in raw_markdown_lines(text, theme) {
        if line.width() == 0 {
            lines.push(Line::from(prefix.to_owned()));
            continue;
        }
        super::ui_transcript_surface::append_prefixed_wrapped_spans_line(
            lines,
            prefix,
            Style::default().fg(theme.markdown.text),
            line.spans,
            width,
        );
    }
}
