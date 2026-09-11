use ratatui::{style::Color, text::Line};

use crate::theme::Theme;

use super::super::ui_streaming_markdown::append_streaming_rich_text_block;
use super::super::ui_transcript_selection::{
    blank_selection_row, selection_rows_for_rendered_line, selection_rows_for_rich_text_block,
    TranscriptSelectionRow,
};
use super::super::ui_transcript_style::blend_color;

pub(super) fn append_reasoning_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &str,
    theme: &Theme,
    surface: Color,
    prefix: &str,
    width: u16,
) -> Vec<TranscriptSelectionRow> {
    // Reasoning shares the answer grammar, syntax palette and wrapping. Blend
    // the rendered colors so nested Markdown and unfinished fences dim too.
    let start = lines.len();
    append_streaming_rich_text_block(lines, body, theme.markdown.text, prefix, theme, width);
    while lines.len() > start
        && lines
            .last()
            .is_some_and(|line| line.to_string().trim().is_empty())
    {
        lines.pop();
    }
    for line in &mut lines[start..] {
        for span in &mut line.spans {
            span.style.fg = span.style.fg.map(|color| blend_color(surface, color, 0.7));
        }
    }
    let rows =
        selection_rows_for_rich_text_block(body, theme.markdown.text, prefix, theme, width, true)
            .unwrap_or_default();
    (0..lines.len() - start)
        .map(|index| {
            rows.get(index).cloned().unwrap_or_else(|| {
                selection_rows_for_rendered_line(&lines[start + index], width)
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| blank_selection_row(width))
            })
        })
        .collect()
}
