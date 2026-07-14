// allow: SIZE_OK — TUI rendering (indivisible view model)
use crate::UnwrapOrAbort;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

use super::ui_chrome::display_width;
use super::ui_diff::render_structured_diff_lines;
use super::ui_fenced_text::{parse_fenced_text_blocks, ParsedTextBlock};
use super::ui_markdown_table::try_render_markdown_table_block;
use super::ui_syntax_highlight::render_highlighted_code_block;
use super::ui_transcript_surface::{
    append_prebuilt_plain_lines, append_prefixed_wrapped_spans_line,
};

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

pub(super) fn parse_inline_markdown_spans(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut pos = 0;

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
                        base_style
                            .fg(theme.markdown.link_text)
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
                base_style
                    .fg(theme.markdown.link)
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
                        base_style.fg(theme.markdown.strong).add_modifier(Modifier::BOLD),
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
                        base_style
                            .fg(theme.text.secondary)
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
                    base_style.fg(theme.markdown.code),
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
                        base_style
                            .fg(theme.markdown.emph)
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
                        base_style
                            .fg(theme.markdown.emph)
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
            spans.push(Span::styled(ch.to_string(), base_style.fg(base_color)));
            pos += ch.len_utf8();
            continue;
        }
        let plain = &remaining[..next_marker];
        spans.push(Span::styled(plain.to_string(), base_style.fg(base_color)));
        pos += next_marker;
    }

    spans
}

pub(super) fn markdown_heading_text(line: &str) -> Option<&str> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    line[hashes..].strip_prefix(' ').map(str::trim)
}

pub(super) fn markdown_rule(line: &str) -> bool {
    let stripped: String = line.chars().filter(|ch| !ch.is_whitespace()).collect();
    matches!(stripped.as_str(), "---" | "***" | "___")
}

pub(super) fn markdown_list_prefix<'a>(
    line: &'a str,
    theme: &Theme,
) -> Option<(String, &'a str, Style, Style)> {
    for marker in ["- [x] ", "* [x] ", "+ [x] "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "☑ ".to_string(),
                text,
                Style::default()
                    .fg(theme.status.success)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    for marker in ["- [ ] ", "* [ ] ", "+ [ ] "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "☐ ".to_string(),
                text,
                Style::default().fg(theme.text.secondary),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    for marker in ["- ", "* ", "+ "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "• ".to_string(),
                text,
                Style::default()
                    .fg(theme.markdown.list_item)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    let digits = line.chars().take_while(|ch| ch.is_ascii_digit()).count();
    if digits > 0 {
        let suffix = &line[digits..];
        if let Some(text) = suffix.strip_prefix(". ") {
            return Some((
                format!("{}{}", &line[..digits], ". "),
                text,
                Style::default()
                    .fg(theme.markdown.list_enum)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.text.primary),
            ));
        }
    }

    None
}

pub(super) fn append_rich_text_block(
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

    let Some(blocks) = parse_fenced_text_blocks(text) else {
        append_markdownish_text_block(lines, text, color, prefix, theme, width);
        return;
    };

    for block in blocks {
        match block {
            ParsedTextBlock::Plain(plain) => {
                append_markdownish_text_block(lines, &plain, color, prefix, theme, width)
            }
            ParsedTextBlock::Code {
                language,
                body,
                raw,
            } => {
                if let Some(language) = language.as_deref() {
                    if matches!(language, "diff" | "patch") {
                        if let Some(diff_lines) =
                            render_structured_diff_lines(&body, None, prefix, width, false, theme)
                        {
                            lines.extend(diff_lines);
                            continue;
                        }
                    }
                }

                let highlighted = render_highlighted_code_block(
                    language.as_deref(),
                    &body,
                    &raw,
                    prefix,
                    color,
                    theme,
                );
                if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty()) {
                    lines.push(Line::default());
                }
                append_prebuilt_plain_lines(lines, prefix, highlighted, width);
                lines.push(Line::default());
                lines.push(Line::default());
            }
        }
    }
}

fn append_markdownish_text_block(
    lines: &mut Vec<Line<'static>>,
    text: &str,
    color: Color,
    prefix: &str,
    theme: &Theme,
    width: u16,
) {
    let base_style = Style::default().fg(color);
    let rows = text.lines().collect::<Vec<_>>();
    let mut index = 0;
    while let Some(line) = rows.get(index).copied() {
        if let Some((table_lines, consumed)) =
            try_render_markdown_table_block(&rows[index..], color, prefix, theme, width)
        {
            lines.extend(table_lines);
            index += consumed;
            continue;
        }

        append_markdownish_line(lines, line, color, prefix, base_style, theme, width);
        index += 1;
    }

    if text.is_empty() {
        append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
    }

    if !lines.is_empty() && !last_line_is_visually_blank(lines) {
        lines.push(Line::default());
    }
}

fn append_markdownish_line(
    lines: &mut Vec<Line<'static>>,
    line: &str,
    color: Color,
    prefix: &str,
    base_style: Style,
    theme: &Theme,
    width: u16,
) {
    if line.is_empty() {
        append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
        return;
    }

    let indent_width = line.chars().take_while(|ch| ch.is_whitespace()).count();
    let indent = " ".repeat(indent_width);
    let trimmed = line.trim_start();
    let content_width = usize::from(width)
        .saturating_sub(display_width(prefix))
        .max(1);

    if let Some(text) = markdown_heading_text(trimmed) {
        if !lines.is_empty() && !last_line_is_visually_blank(lines) {
            lines.push(Line::default());
        }
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown_spans(
                text,
                base_style
                    .fg(theme.markdown.heading)
                    .add_modifier(Modifier::BOLD),
                theme.markdown.heading,
                theme,
            ),
            width,
        );
        return;
    }

    if markdown_rule(trimmed) {
        append_prefixed_wrapped_spans_line(
            lines,
            prefix,
            base_style,
            vec![Span::styled(
                "─".repeat(content_width),
                Style::default().fg(theme.markdown.rule),
            )],
            width,
        );
        return;
    }

    if let Some(text) = trimmed.strip_prefix("> ") {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.markdown.block_quote),
            parse_inline_markdown_spans(
                text,
                Style::default()
                    .fg(theme.markdown.block_quote)
                    .add_modifier(Modifier::ITALIC),
                theme.markdown.block_quote,
                theme,
            ),
            width,
        );
        return;
    }

    if let Some((list_prefix, text, list_style, text_style)) = markdown_list_prefix(trimmed, theme)
    {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}{list_prefix}"),
            list_style,
            parse_inline_markdown_spans(text, text_style, color, theme),
            width,
        );
        return;
    }

    append_prefixed_wrapped_spans_line(
        lines,
        prefix,
        base_style,
        parse_inline_markdown_spans(trimmed, base_style, color, theme),
        width,
    );
}

pub(super) fn raw_url_length(text: &str) -> Option<usize> {
    let prefix = if text.starts_with("https://") {
        "https://"
    } else if text.starts_with("http://") {
        "http://"
    } else {
        return None;
    };

    let tail = &text[prefix.len()..];
    let extra = tail.find(char::is_whitespace).unwrap_or(tail.len());
    Some(prefix.len() + extra)
}

fn last_line_is_visually_blank(lines: &[Line<'static>]) -> bool {
    lines.last().is_some_and(|line| {
        line.spans.is_empty()
            || line
                .spans
                .iter()
                .all(|span| span.content.chars().all(char::is_whitespace))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect_spans<'a>(lines: &'a [Line<'static>]) -> Vec<&'a Span<'static>> {
        lines.iter().flat_map(|line| line.spans.iter()).collect()
    }

    #[test]
    fn bold_uses_markdown_strong_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("**bold**", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.markdown.strong));
        assert!(spans[0].style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn italic_asterisk_uses_markdown_emph_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("*italic*", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.markdown.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn italic_underscore_uses_markdown_emph_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("_italic_", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.markdown.emph));
        assert!(spans[0].style.add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn inline_code_uses_markdown_code_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("`code`", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.markdown.code));
    }

    #[test]
    fn link_label_uses_markdown_link_text_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans =
            parse_inline_markdown_spans("[label](https://example.com)", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "label");
        assert_eq!(spans[0].style.fg, Some(theme.markdown.link_text));
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn raw_url_uses_markdown_link_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans(
            "see https://example.com now",
            base,
            theme.text.primary,
            &theme,
        );
        assert!(spans.iter().any(|s| {
            s.style.fg == Some(theme.markdown.link)
                && s.style.add_modifier.contains(Modifier::UNDERLINED)
        }));
    }

    #[test]
    fn strikethrough_uses_text_secondary_color() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("~~deleted~~", base, theme.text.primary, &theme);
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].style.fg, Some(theme.text.secondary));
        assert!(spans[0].style.add_modifier.contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn intraword_asterisks_not_emphasized() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("foo*bar*baz", base, theme.text.primary, &theme);
        for span in &spans {
            assert_eq!(span.style.fg, Some(theme.text.primary));
            assert!(!span.style.add_modifier.contains(Modifier::ITALIC));
        }
    }

    #[test]
    fn intraword_underscores_not_emphasized() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("foo_bar_baz", base, theme.text.primary, &theme);
        for span in &spans {
            assert_eq!(span.style.fg, Some(theme.text.primary));
            assert!(!span.style.add_modifier.contains(Modifier::ITALIC));
        }
    }

    #[test]
    fn intraword_underscores_in_identifiers_not_emphasized() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans(
            "background_output session_search",
            base,
            theme.text.primary,
            &theme,
        );
        for span in &spans {
            assert_eq!(span.style.fg, Some(theme.text.primary));
            assert!(!span.style.add_modifier.contains(Modifier::ITALIC));
        }
    }

    #[test]
    fn heading_uses_markdown_heading_color() {
        let theme = Theme::default();
        let mut lines = Vec::new();
        append_rich_text_block(&mut lines, "# Heading", theme.text.primary, "", &theme, 80);
        let spans = collect_spans(&lines);
        assert!(
            spans.iter().any(|span| {
                span.style.fg == Some(theme.markdown.heading)
                    && span.style.add_modifier.contains(Modifier::BOLD)
            }),
            "heading should use theme.markdown.heading with BOLD, got: {spans:?}"
        );
    }

    #[test]
    fn blockquote_uses_markdown_block_quote_color() {
        let theme = Theme::default();
        let mut lines = Vec::new();
        append_rich_text_block(&mut lines, "> Quote", theme.text.primary, "", &theme, 80);
        let spans = collect_spans(&lines);
        assert!(
            spans.iter().any(|span| {
                span.style.fg == Some(theme.markdown.block_quote)
                    && span.style.add_modifier.contains(Modifier::ITALIC)
            }),
            "blockquote should use theme.markdown.block_quote with ITALIC, got: {spans:?}"
        );
    }

    #[test]
    fn rule_uses_markdown_rule_color() {
        let theme = Theme::default();
        let mut lines = Vec::new();
        append_rich_text_block(&mut lines, "---", theme.text.primary, "", &theme, 80);
        let spans = collect_spans(&lines);
        assert!(
            spans.iter().any(|span| span.style.fg == Some(theme.markdown.rule)),
            "rule should use theme.markdown.rule, got: {spans:?}"
        );
    }

    #[test]
    fn bullet_marker_uses_markdown_list_item_color() {
        let theme = Theme::default();
        let (prefix, _, marker_style, _) = markdown_list_prefix("- item", &theme).unwrap();
        assert_eq!(prefix, "• ");
        assert_eq!(marker_style.fg, Some(theme.markdown.list_item));
        assert!(marker_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn enum_marker_uses_markdown_list_enum_color() {
        let theme = Theme::default();
        let (prefix, _, marker_style, _) = markdown_list_prefix("1. item", &theme).unwrap();
        assert_eq!(prefix, "1. ");
        assert_eq!(marker_style.fg, Some(theme.markdown.list_enum));
        assert!(marker_style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn table_header_uses_markdown_heading_color() {
        let theme = Theme::default();
        let rows = ["Name | Value", "--- | ---", "foo | bar"];
        let (lines, _) =
            try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 80).unwrap();
        let spans = collect_spans(&lines);
        assert!(
            spans.iter().any(|span| {
                span.style.fg == Some(theme.markdown.heading)
                    && span.style.add_modifier.contains(Modifier::BOLD)
            }),
            "table header should use theme.markdown.heading with BOLD, got: {spans:?}"
        );
    }
}
