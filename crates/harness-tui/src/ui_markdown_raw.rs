use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::theme::Theme;
#[path = "ui_markdown_raw/highlight.rs"]
mod highlight;

#[cfg(test)]
#[path = "ui_markdown_raw/tests.rs"]
mod tests;

pub(super) fn raw_markdown_lines(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    highlight::render(text, theme)
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
            lines.push(Line::from(vec![
                Span::raw(prefix.to_owned()),
                Span::styled("", line.style),
            ]));
            continue;
        }
        super::ui_markdown::append_markdown_spans_line(
            lines,
            prefix,
            Style::default().fg(theme.markdown.text),
            line.spans,
            width,
            theme,
        );
    }
}
