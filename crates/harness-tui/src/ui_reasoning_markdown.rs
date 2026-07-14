// allow: SIZE_OK — reasoning-body markdown rendering (block-level handler + inline parser share one color struct)
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use crate::UnwrapOrAbort;

use super::*;

const REASONING_OPACITY: f32 = 0.6;

pub(super) struct ReasoningMarkdownColors {
    base: Color,
    heading: Color,
    link: Color,
    link_text: Color,
    code: Color,
    emph: Color,
    strong: Color,
    strikethrough: Color,
    block_quote: Color,
    list_marker: Color,
    list_enum: Color,
    rule: Color,
}

pub(super) fn reasoning_markdown_colors(theme: &Theme, surface: Color) -> ReasoningMarkdownColors {
    let blend = |overlay: Color| blend_color(surface, overlay, REASONING_OPACITY);
    ReasoningMarkdownColors {
        base: theme.text.secondary,
        heading: blend(theme.markdown.heading),
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

pub(super) fn append_reasoning_body_lines(
    lines: &mut Vec<Line<'static>>,
    body: &str,
    theme: &Theme,
    surface: Color,
    width: u16,
) {
    let colors = reasoning_markdown_colors(theme, surface);
    let base_style = Style::default().fg(colors.base);

    for row in body.lines() {
        if row.is_empty() {
            append_prefixed_wrapped_spans_line(
                lines,
                TRANSCRIPT_REASONING_BODY_PREFIX,
                base_style,
                Vec::new(),
                width,
            );
            continue;
        }

        let trimmed = row.trim_start();
        let indent = &row[..row.len() - trimmed.len()];

        if let Some(heading_text) = markdown_heading_text(trimmed) {
            let heading_style = Style::default()
                .fg(colors.heading)
                .add_modifier(Modifier::BOLD);
            let spans = parse_reasoning_inline_spans(heading_text, &colors);
            append_prefixed_wrapped_spans_line(
                lines,
                &format!("{TRANSCRIPT_REASONING_BODY_PREFIX}{indent}"),
                heading_style,
                spans,
                width,
            );
            continue;
        }

        if markdown_rule(trimmed) {
            let content_width = usize::from(width)
                .saturating_sub(TRANSCRIPT_REASONING_BODY_PREFIX.len())
                .max(1);
            append_prefixed_wrapped_spans_line(
                lines,
                TRANSCRIPT_REASONING_BODY_PREFIX,
                base_style,
                vec![Span::styled(
                    "─".repeat(content_width),
                    Style::default().fg(colors.rule),
                )],
                width,
            );
            continue;
        }

        if let Some(text) = trimmed.strip_prefix("> ") {
            let quote_style = Style::default()
                .fg(colors.block_quote)
                .add_modifier(Modifier::ITALIC);
            let spans = parse_reasoning_inline_spans(text, &colors);
            append_prefixed_wrapped_spans_line(
                lines,
                &format!("{TRANSCRIPT_REASONING_BODY_PREFIX}{indent}▍ "),
                quote_style,
                spans,
                width,
            );
            continue;
        }

        if let Some((marker, text, is_enum)) = parse_list_prefix(trimmed) {
            let marker_color = if is_enum {
                colors.list_enum
            } else {
                colors.list_marker
            };
            let marker_style = Style::default()
                .fg(marker_color)
                .add_modifier(Modifier::BOLD);
            let spans = parse_reasoning_inline_spans(text, &colors);
            append_prefixed_wrapped_spans_line(
                lines,
                &format!("{TRANSCRIPT_REASONING_BODY_PREFIX}{indent}{marker}"),
                marker_style,
                spans,
                width,
            );
            continue;
        }

        let spans = parse_reasoning_inline_spans(trimmed, &colors);
        append_prefixed_wrapped_spans_line(
            lines,
            TRANSCRIPT_REASONING_BODY_PREFIX,
            base_style,
            spans,
            width,
        );
    }
}

fn parse_list_prefix(line: &str) -> Option<(String, &str, bool)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some(("• ".to_string(), text, false));
        }
    }
    let digits = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 {
        if let Some(text) = line[digits..].strip_prefix(". ") {
            return Some((format!("{}. ", &line[..digits]), text, true));
        }
    }
    None
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

fn parse_reasoning_inline_spans(
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
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("hello world", &colors);
        assert_eq!(spans.len(), 1);
        assert!(span_is_plain(&spans[0], &colors));
        assert!(!spans[0].style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn intraword_underscores_are_not_emphasis() {
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
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("**important**", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.strong));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn emphasis_asterisks_render_with_emph_color() {
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("*italic*", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn emphasis_underscores_render_with_emph_color() {
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("_italic_", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_code_renders_with_code_color() {
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("`code`", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.code));
    }

    #[test]
    fn strikethrough_renders_with_crossed_out() {
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("~~deleted~~", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(colors.strikethrough));
        assert!(spans[0].style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn intraword_asterisks_are_not_emphasis() {
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
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("[label](https://example.com)", &colors);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "label");
        assert_eq!(spans[0].style.fg, Some(colors.link_text));
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn raw_url_renders_with_link_color() {
        let colors = test_colors();
        let spans = parse_reasoning_inline_spans("see https://example.com for details", &colors);
        assert!(spans.iter().any(|s| {
            s.style.fg == Some(colors.link) && s.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
    }

    #[test]
    fn blended_colors_differ_from_raw_theme_colors() {
        let theme = Theme::default();
        let colors = reasoning_markdown_colors(&theme, theme.surface.shell);
        assert_ne!(colors.heading, theme.markdown.heading);
        assert_ne!(colors.code, theme.markdown.code);
        assert_eq!(colors.base, theme.text.secondary);
    }
}
