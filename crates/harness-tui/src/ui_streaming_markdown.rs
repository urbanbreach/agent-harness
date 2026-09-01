use ratatui::{style::Color, text::Line};

use crate::theme::Theme;

use super::ui_fenced_text::{parse_streaming_fenced_text_blocks, ParsedTextBlock};
use super::ui_markdown::append_markdownish_text_block;
use super::ui_syntax_highlight::render_highlighted_code_block;
use super::ui_transcript_surface::append_prebuilt_plain_lines;

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
            ParsedTextBlock::Code {
                language,
                body,
                raw,
            } => {
                append_streaming_code(
                    lines,
                    language.as_deref(),
                    &body,
                    &raw,
                    prefix,
                    theme,
                    width,
                );
            }
        }
    }
}

fn append_streaming_code(
    lines: &mut Vec<Line<'static>>,
    language: Option<&str>,
    body: &str,
    raw: &str,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    if !lines.is_empty() && !lines.last().is_some_and(line_is_blank) {
        lines.push(Line::default());
    }
    let highlighted =
        render_highlighted_code_block(language, body, raw, prefix, theme.markdown.text, theme);
    append_prebuilt_plain_lines(lines, prefix, highlighted, width);
    lines.push(Line::default());
    lines.push(Line::default());
}

fn line_is_blank(line: &Line<'static>) -> bool {
    line.spans
        .iter()
        .all(|span| span.content.chars().all(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use ratatui::{style::Color, text::Line};

    use super::append_streaming_rich_text_block;
    use crate::theme::Theme;
    use crate::ui::ui_markdown::append_rich_text_block;

    fn code_rows<'a>(lines: &'a [Line<'static>]) -> Vec<&'a Line<'static>> {
        lines
            .iter()
            .filter(|line| line.spans.iter().any(|span| !span.content.is_empty()))
            .collect()
    }

    #[test]
    fn open_rust_fence_highlights_each_appended_chunk_and_keeps_prior_rows_stable() {
        // Given: two cumulative snapshots of an unfinished Rust fence.
        let theme = Theme::default();
        let mut first = Vec::new();
        let mut second = Vec::new();

        // When: another source row is appended to the stream.
        append_streaming_rich_text_block(
            &mut first,
            "```rust\nfn main() {",
            Color::Gray,
            "  ",
            &theme,
            40,
        );
        append_streaming_rich_text_block(
            &mut second,
            "```rust\nfn main() {\n    let answer = 42;",
            Color::Gray,
            "  ",
            &theme,
            40,
        );

        // Then: syntax colors are incremental and the already-painted row is unchanged.
        let first_code = code_rows(&first);
        let second_code = code_rows(&second);
        assert_eq!(first_code[0], second_code[0]);
        assert!(second_code.iter().flat_map(|line| &line.spans).any(|span| {
            span.style
                .fg
                .is_some_and(|color| color != theme.markdown.code)
        }));
    }

    #[test]
    fn open_fence_handles_crlf_unknown_and_malformed_languages_without_geometry_drift() {
        // Given: open fences with CRLF, unknown, and malformed language tokens.
        let theme = Theme::default();
        for source in [
            "```unknown\r\nalpha\r\nbeta",
            "````rust\nalpha\nbeta",
            "```\r\nalpha\r\nbeta",
        ] {
            let mut lines = Vec::new();

            // When: each source streams through the production renderer.
            append_streaming_rich_text_block(&mut lines, source, Color::Blue, "  ", &theme, 12);

            // Then: source rows remain ordered and width-bounded without exposing CR bytes.
            let text = lines
                .iter()
                .flat_map(|line| &line.spans)
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert!(text.contains("alpha") && text.contains("beta"), "{text:?}");
            assert!(!text.contains('\r'));
            assert!(lines.iter().all(|line| line.width() <= 12));
        }
    }

    #[test]
    fn closing_plain_fence_preserves_streamed_code_row_geometry_and_styles() {
        // Given: the same Rust block before and after its closing fence arrives.
        let theme = Theme::default();
        let mut streaming = Vec::new();
        let mut settled = Vec::new();
        append_streaming_rich_text_block(
            &mut streaming,
            "```rust\nfn main() {\n    let answer = 42;",
            Color::Gray,
            "  ",
            &theme,
            40,
        );

        // When: the block settles through the structural renderer.
        append_rich_text_block(
            &mut settled,
            "```rust\nfn main() {\n    let answer = 42;\n```",
            Color::Gray,
            "  ",
            &theme,
            40,
        );

        // Then: settling does not reflow or recolor prior code rows.
        assert_eq!(code_rows(&streaming), code_rows(&settled));
    }
}
