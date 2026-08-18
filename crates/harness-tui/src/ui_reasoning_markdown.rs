// allow: SIZE_OK — reasoning-body markdown rendering (block-level handler + inline parser share one color struct)
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use crate::UnwrapOrAbort;

use super::*;

const REASONING_OPACITY: f32 = 0.7;

pub(super) struct ReasoningMarkdownColors {
    pub(super) base: Color,
    pub(super) heading: Color,
    pub(super) link: Color,
    pub(super) link_text: Color,
    pub(super) code: Color,
    pub(super) emph: Color,
    pub(super) strong: Color,
    pub(super) strikethrough: Color,
    pub(super) block_quote: Color,
    pub(super) list_marker: Color,
    pub(super) list_enum: Color,
    pub(super) rule: Color,
}

pub(super) fn reasoning_markdown_colors(theme: &Theme, surface: Color) -> ReasoningMarkdownColors {
    let blend = |overlay: Color| blend_color(surface, overlay, REASONING_OPACITY);
    ReasoningMarkdownColors {
        base: blend(theme.markdown.text),
        heading: blend(theme.markdown.heading_h1),
        link: blend(theme.markdown.link),
        link_text: blend(theme.markdown.link_text),
        code: blend(theme.markdown.code),
        emph: blend(theme.markdown.emph),
        strong: blend(theme.markdown.strong),
        strikethrough: blend(theme.text.secondary),
        block_quote: blend(theme.markdown.block_quote),
        list_marker: blend(theme.markdown.list_item),
        list_enum: blend(theme.markdown.list_enum),
        rule: blend(theme.markdown.rule),
    }
}

fn is_flanking_pair(prev: Option<char>, content: &str, after_close: &str) -> bool {
    !content.is_empty()
        && !prev.is_some_and(char::is_alphanumeric)
        && !content.starts_with(char::is_whitespace)
        && !content.ends_with(char::is_whitespace)
        && !after_close
            .chars()
            .next()
            .is_some_and(char::is_alphanumeric)
}

pub(super) fn parse_reasoning_inline_spans(
    text: &str,
    colors: &ReasoningMarkdownColors,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut pos = 0;
    let base_style = Style::default().fg(colors.base);

    while pos < text.len() {
        let remaining = &text[pos..];
        let prev = if pos > 0 {
            text[..pos].chars().next_back()
        } else {
            None
        };

        if let Some(rest) = remaining.strip_prefix('[') {
            if let Some(label_end) = rest.find("](") {
                let after_label = &rest[label_end + 2..];
                if let Some(url_end) = after_label.find(')') {
                    spans.push(Span::styled(
                        rest[..label_end].to_string(),
                        Style::default()
                            .fg(colors.link_text)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    pos += 1 + label_end + 2 + url_end + 1;
                    continue;
                }
            }
        }

        if let Some(url_len) = raw_url_length(remaining) {
            spans.push(Span::styled(
                remaining[..url_len].to_string(),
                Style::default()
                    .fg(colors.link)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            pos += url_len;
            continue;
        }

        if let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                let content = &rest[..end];
                if is_flanking_pair(prev, content, &rest[end + 2..]) {
                    spans.push(Span::styled(
                        content.to_string(),
                        Style::default()
                            .fg(colors.strong)
                            .add_modifier(Modifier::BOLD),
                    ));
                    pos += 2 + end + 2;
                    continue;
                }
            }
        }

        if let Some(rest) = remaining.strip_prefix("~~") {
            if let Some(end) = rest.find("~~") {
                let content = &rest[..end];
                if is_flanking_pair(prev, content, &rest[end + 2..]) {
                    spans.push(Span::styled(
                        content.to_string(),
                        Style::default()
                            .fg(colors.strikethrough)
                            .add_modifier(Modifier::CROSSED_OUT),
                    ));
                    pos += 2 + end + 2;
                    continue;
                }
            }
        }

        if let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    Style::default().fg(colors.code),
                ));
                pos += 1 + end + 1;
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                let content = &rest[..end];
                if is_flanking_pair(prev, content, &rest[end + 1..]) {
                    spans.push(Span::styled(
                        content.to_string(),
                        Style::default()
                            .fg(colors.emph)
                            .add_modifier(Modifier::ITALIC),
                    ));
                    pos += 1 + end + 1;
                    continue;
                }
            }
        }

        if let Some(rest) = remaining.strip_prefix('_') {
            if let Some(end) = rest.find('_') {
                let content = &rest[..end];
                if is_flanking_pair(prev, content, &rest[end + 1..]) {
                    spans.push(Span::styled(
                        content.to_string(),
                        Style::default()
                            .fg(colors.emph)
                            .add_modifier(Modifier::ITALIC),
                    ));
                    pos += 1 + end + 1;
                    continue;
                }
            }
        }

        let next_marker = ["[", "http://", "https://", "**", "~~", "`", "*", "_"]
            .into_iter()
            .filter_map(|marker| remaining.find(marker))
            .min()
            .unwrap_or(remaining.len());
        if next_marker == 0 {
            let ch = remaining.chars().next().unwrap_or_abort();
            spans.push(Span::styled(ch.to_string(), base_style));
            pos += ch.len_utf8();
            continue;
        }
        spans.push(Span::styled(
            remaining[..next_marker].to_string(),
            base_style,
        ));
        pos += next_marker;
    }

    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_colors() -> ReasoningMarkdownColors {
        let theme = Theme::default();
        reasoning_markdown_colors(&theme, theme.surface.shell)
    }

    fn span_is_plain(span: &Span, colors: &ReasoningMarkdownColors) -> bool {
        span.style.fg == Some(colors.base) && span.style.add_modifier == Modifier::empty()
    }

    #[test]
    fn plain_text_uses_base_color_without_dim() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("hello world", &colors);
        assert_eq!(spans.len(), 1);
        assert!(span_is_plain(&spans[0], &colors));
        assert!(!spans[0].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn intraword_underscores_are_not_emphasis() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans =
            parse_reasoning_inline_spans("background_output session_search gh_grep", &colors);
        for span in &spans {
            assert!(
                span_is_plain(span, &colors),
                "span {:?} should be plain text, got fg={:?} modifier={:?}",
                span.content,
                span.style.fg,
                span.style.add_modifier
            );
        }
    }

    #[test]
    fn screenshot_text_has_no_false_positives() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let text = "18. sessionlist, sessionread, sessionsearch, sessioninfo - session tools\n\
                    19. backgroundoutput, backgroundcancel - background task tools\n\
                    20. mcp tools (docs-rs, gh_grep)";
        for line in text.lines() {
            let spans = parse_reasoning_inline_spans(line, &colors);
            for span in &spans {
                assert!(
                    span_is_plain(span, &colors),
                    "span {:?} should be plain text in line {:?}, got fg={:?} modifier={:?}",
                    span.content,
                    line,
                    span.style.fg,
                    span.style.add_modifier
                );
            }
        }
    }

    #[test]
    fn bold_delimiters_render_with_strong_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("**important**", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.strong));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn emphasis_asterisks_render_with_emph_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("*italic*", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn emphasis_underscores_render_with_emph_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("_italic_", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_code_renders_with_code_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("`code`", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.code));
    }

    #[test]
    fn strikethrough_renders_with_crossed_out() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("~~deleted~~", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.strikethrough));
        assert!(spans[0].style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn intraword_asterisks_are_not_emphasis() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("foo*bar*baz", &colors);
        for span in &spans {
            assert!(
                span_is_plain(span, &colors),
                "span {:?} should be plain text, got fg={:?} modifier={:?}",
                span.content,
                span.style.fg,
                span.style.add_modifier
            );
        }
    }

    #[test]
    fn link_renders_with_link_text_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("[label](https://example.com)", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "label");
        assert_eq!(spans[0].style.fg, Some(colors.link_text));
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn raw_url_renders_with_link_color() {
        // arrange
        // act
        // assert
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("see https://example.com for details", &colors);
        assert!(spans.iter().any(|s| {
            s.style.fg == Some(colors.link) && s.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
    }

    #[test]
    fn reasoning_colors_match_groks_seventy_percent_background_blend() {
        // arrange
        // act
        // assert
        let theme = Theme::default();
        let colors = reasoning_markdown_colors(&theme, theme.surface.shell);
        assert_eq!(
            colors.base,
            blend_color(theme.surface.shell, theme.markdown.text, 0.7)
        );
        assert_eq!(
            colors.heading,
            blend_color(theme.surface.shell, theme.markdown.heading_h1, 0.7)
        );
        assert_eq!(
            colors.code,
            blend_color(theme.surface.shell, theme.markdown.code, 0.7)
        );
    }
}
