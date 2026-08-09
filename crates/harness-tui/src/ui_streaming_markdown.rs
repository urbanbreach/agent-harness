use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

use super::ui_fenced_text::{parse_streaming_fenced_text_blocks, ParsedTextBlock};
use super::ui_markdown::append_markdownish_text_block;
use super::ui_transcript_surface::append_prefixed_wrapped_spans_line;

pub(super) fn append_streaming_rich_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    if !text.contains("```") {
        append_markdownish_text_block(lines, text, color, prefix, theme, width);
        return;
    }

    for block in parse_streaming_fenced_text_blocks(text) {
        match block {
            ParsedTextBlock::Plain(plain) => {
                append_markdownish_text_block(lines, &plain, color, prefix, theme, width);
            }
            ParsedTextBlock::Code { body, .. } => {
                append_streaming_code(lines, &body, prefix, theme, width);
            }
        }
    }
}

fn append_streaming_code(
    lines: &mut Vec<Line<'static>>,
    body: &str,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    if !lines.is_empty() && !lines.last().is_some_and(line_is_blank) {
        lines.push(Line::default());
    }
    let style = Style::default()
        .fg(theme.markdown.code)
        .bg(theme.markdown.code_background);
    for source_line in body.split('\n') {
        append_prefixed_wrapped_spans_line(
            lines,
            prefix,
            Style::default(),
            vec![Span::styled(source_line.to_string(), style)],
            width,
        );
    }
    lines.push(Line::default());
    lines.push(Line::default());
}

fn line_is_blank(line: &Line<'static>) -> bool {
    line.spans
        .iter()
        .all(|span| span.content.chars().all(char::is_whitespace))
}
