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
use super::ui_transcript_mermaid::{is_mermaid_language, render_mermaid_diagram};
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

fn markdown_link_destination_end(destination: &str) -> Option<usize> {
    let mut nested_parentheses = 0usize;
    let mut escaped = false;
    for (index, ch) in destination.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '(' => nested_parentheses = nested_parentheses.saturating_add(1),
            ')' if nested_parentheses == 0 => return Some(index),
            ')' => nested_parentheses = nested_parentheses.saturating_sub(1),
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InlineMarkdownLink {
    pub(super) label: String,
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
}

#[derive(Debug, Clone)]
pub(super) struct ParsedInlineMarkdown {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<InlineMarkdownLink>,
}

pub(super) fn parse_inline_markdown(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> ParsedInlineMarkdown {
    let spans = parse_inline_markdown_spans(text, base_style, base_color, theme);
    let mut links = Vec::new();
    let mut position = 0;
    while position < text.len() {
        let remaining = &text[position..];
        if let Some(rest) = remaining.strip_prefix('[') {
            if let Some(label_end) = rest.find("](") {
                let destination_start = label_end + 2;
                if let Some(destination_end) =
                    markdown_link_destination_end(&rest[destination_start..])
                {
                    let destination = &rest[destination_start..destination_start + destination_end];
                    let label = parse_inline_markdown_spans(
                        &rest[..label_end],
                        base_style,
                        base_color,
                        theme,
                    )
                    .into_iter()
                    .map(|span| span.content.into_owned())
                    .collect::<String>();
                    let start_cell = parse_inline_markdown_spans(
                        &text[..position],
                        base_style,
                        base_color,
                        theme,
                    )
                    .iter()
                    .map(Span::width)
                    .sum();
                    let label_width = display_width(&label);
                    if crate::transcript_selection::Hyperlink::new(
                        &label,
                        destination,
                        crate::transcript_selection::LinkRange::new(
                            0,
                            start_cell,
                            start_cell.saturating_add(label_width.saturating_sub(1)),
                        ),
                    )
                    .is_ok()
                    {
                        links.push(InlineMarkdownLink {
                            label,
                            start_cell,
                            end_cell: start_cell.saturating_add(label_width),
                            destination: destination.to_string(),
                        });
                    }
                    position += 1 + destination_start + destination_end + 1;
                    continue;
                }
            }
        }
        if let Some(url_len) = raw_url_length(remaining) {
            let destination = &remaining[..url_len];
            let start_cell =
                parse_inline_markdown_spans(&text[..position], base_style, base_color, theme)
                    .iter()
                    .map(Span::width)
                    .sum();
            let destination_width = display_width(destination);
            if crate::transcript_selection::Hyperlink::new(
                destination,
                destination,
                crate::transcript_selection::LinkRange::new(
                    0,
                    start_cell,
                    start_cell.saturating_add(destination_width.saturating_sub(1)),
                ),
            )
            .is_ok()
            {
                links.push(InlineMarkdownLink {
                    label: destination.to_string(),
                    start_cell,
                    end_cell: start_cell.saturating_add(destination_width),
                    destination: destination.to_string(),
                });
            }
            position += url_len;
            continue;
        }
        position += remaining.chars().next().map_or(1, char::len_utf8);
    }
    ParsedInlineMarkdown { spans, links }
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
                if let Some(url_end) = markdown_link_destination_end(after_label) {
                    let link_style = base_style
                        .fg(theme.markdown.link_text)
                        .add_modifier(Modifier::UNDERLINED);
                    spans.extend(parse_inline_markdown_spans(
                        &rest[..label_end],
                        link_style,
                        theme.markdown.link_text,
                        theme,
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
                        base_style
                            .fg(theme.markdown.strong)
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
    markdown_heading(line).map(|(_, text)| text)
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|ch| *ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    line[hashes..]
        .strip_prefix(' ')
        .map(str::trim)
        .map(|text| (hashes, text))
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
                    .fg(theme.markdown.task_checked)
                    .add_modifier(Modifier::BOLD),
                Style::default().fg(theme.markdown.text),
            ));
        }
    }

    for marker in ["- [ ] ", "* [ ] ", "+ [ ] "] {
        if let Some(text) = line.strip_prefix(marker) {
            return Some((
                "☐ ".to_string(),
                text,
                Style::default().fg(theme.markdown.task_unchecked),
                Style::default().fg(theme.markdown.text),
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
                Style::default().fg(theme.markdown.text),
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
                Style::default().fg(theme.markdown.text),
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

                if is_mermaid_language(language.as_deref()) {
                    if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty())
                    {
                        lines.push(Line::default());
                    }
                    lines.extend(render_mermaid_diagram(&body, prefix, theme, width));
                    lines.push(Line::default());
                    lines.push(Line::default());
                    continue;
                }

                let highlighted = render_highlighted_code_block(
                    language.as_deref(),
                    &body,
                    &raw,
                    prefix,
                    theme.markdown.text,
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

pub(super) fn append_markdownish_text_block(
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
        if let Some((table_lines, consumed, _links)) =
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

    if let Some((level, text)) = markdown_heading(trimmed) {
        let heading_color = theme.markdown.heading(level);
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
                    .fg(heading_color)
                    .add_modifier(crate::theme::MarkdownColors::heading_modifier(level)),
                heading_color,
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
        let spans = parse_inline_markdown_spans(
            "[label](https://example.com)",
            base,
            theme.text.primary,
            &theme,
        );
        assert_eq!(spans.len(), 1);
        assert_eq!(spans[0].content.as_ref(), "label");
        assert_eq!(spans[0].style.fg, Some(theme.markdown.link_text));
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn inline_parser_preserves_safe_destinations_and_rejects_unsafe_targets() {
        // Given: labeled links, a raw URL, and executable/local destinations.
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);

        // When: inline markdown crosses the rendering boundary.
        let parsed = parse_inline_markdown(
            "[docs](https://example.com/docs) http://example.com/raw [nested](https://example.com/a_(b)) [bad](javascript:alert(1)) [file](file:///tmp/x)",
            base,
            theme.text.primary,
            &theme,
        );

        // Then: safe raw destinations survive unchanged and unsafe targets carry no metadata.
        assert_eq!(
            parsed
                .links
                .iter()
                .map(|link| (link.label.as_str(), link.destination.as_str()))
                .collect::<Vec<_>>(),
            vec![
                ("docs", "https://example.com/docs"),
                ("http://example.com/raw", "http://example.com/raw"),
                ("nested", "https://example.com/a_(b)"),
            ]
        );
        let visible = parsed
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert_eq!(visible, "docs http://example.com/raw nested bad file");
        assert!(!visible.contains("javascript:") && !visible.contains("file:///"));
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
                span.style.fg == Some(theme.markdown.heading_h1)
                    && span.style.add_modifier.contains(Modifier::BOLD)
            }),
            "heading should use theme.markdown.heading with BOLD, got: {spans:?}"
        );
    }

    #[test]
    fn mermaid_flowchart_renders_unicode_nodes_instead_of_a_placeholder() {
        // arrange
        // act
        let text =
            render_mermaid_diagram("graph TD\n  A[Start] --> B[End]", "", &Theme::default(), 80)
                .into_iter()
                .flat_map(|line| line.spans)
                .map(|span| span.content.into_owned())
                .collect::<Vec<_>>()
                .join("\n");

        // assert
        assert!(
            text.contains("┌") && text.contains("Start") && text.contains('▼'),
            "{text}"
        );
        assert!(!text.contains("Mermaid graph"), "{text}");
    }

    #[test]
    fn mermaid_flowchart_reuses_labeled_nodes_across_edges() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "graph TD\n  A[Start] --> B[Build]\n  B --> C[Done]",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Start") && text.contains("Build") && text.contains("Done"),
            "{text}"
        );
        assert!(
            !text.lines().any(|line| line.contains("│   B   │")),
            "{text}"
        );
    }

    #[test]
    fn mermaid_sequence_renders_lifelines_without_source_syntax() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "sequenceDiagram\n  Alice->>Bob: Hello",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Alice") && text.contains("Bob") && text.contains('▶'),
            "{text}"
        );
        assert!(
            !text.contains("sequenceDiagram") && !text.contains("Alice->>Bob"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_state_diagram_renders_nodes_without_source_syntax() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "stateDiagram-v2\n  [*] --> Active\n  Active --> Done",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Active") && text.contains("Done") && text.contains('▼'),
            "{text}"
        );
        assert!(
            !text.contains("stateDiagram-v2") && !text.contains("[*] -->"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_class_diagram_renders_members_without_source_syntax() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "classDiagram\nclass Animal {\n  +int age\n  +isMammal() bool\n}\nAnimal <|-- Duck",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Animal")
                && text.contains("Duck")
                && text.contains("+int age")
                && text.contains("+isMammal() bool"),
            "{text}"
        );
        assert!(text.contains('├') && text.contains('△'), "{text}");
        assert!(!text.contains("classDiagram"), "{text}");
    }

    #[test]
    fn mermaid_entity_relationship_diagram_renders_cardinality_and_attributes() {
        let text = render_mermaid_diagram(
            "erDiagram\nCUSTOMER ||--o{ ORDER : places\nCUSTOMER {\n  string name PK \"full name\"\n}",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        assert!(
            text.contains("CUSTOMER")
                && text.contains("ORDER")
                && text.contains("string name PK")
                && text.contains("1 places 0..*"),
            "{text}"
        );
        assert!(
            !text.contains("erDiagram") && !text.contains("full name"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_sequence_renders_declared_participants_and_control_rows() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "sequenceDiagram\nparticipant C as Client\nparticipant S as Server\nautonumber\nC->>S: GET /\nNote over C,S: happy path\nloop retry\nS-->>C: ok\nend",
            "",
            &Theme::default(),
            100,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Client")
                && text.contains("Server")
                && text.contains("1. GET /")
                && text.contains("happy path")
                && text.contains("loop retry")
                && text.contains(" end "),
            "{text}"
        );
        assert!(
            !text.contains("sequenceDiagram") && !text.contains("C->>S"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_flowchart_renders_group_and_relationship_label_without_source_syntax() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "flowchart TD\nsubgraph workers [Workers]\nA[Start] -->|dispatch| B[Build]\nend\nB --> C[Done]",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Workers")
                && text.contains("Start")
                && text.contains("Build")
                && text.contains("Done")
                && text.contains("dispatch"),
            "{text}"
        );
        assert!(
            !text.contains("subgraph") && !text.contains("A[Start]"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_state_choice_renders_as_a_diamond_without_source_syntax() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "stateDiagram-v2\nstate c <<choice>>\nIdle --> c\nc --> Done: yes",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains('◇') && text.contains("Idle") && text.contains("Done"),
            "{text}"
        );
        assert!(
            !text.contains("<<choice>>") && !text.contains("state c"),
            "{text}"
        );
    }

    #[test]
    fn mermaid_state_alias_uses_its_display_name_and_skips_notes() {
        // arrange
        // act
        let text = render_mermaid_diagram(
            "stateDiagram-v2\nstate \"Waiting for input\" as W\nW --> Done\nnote right of W: internal detail\nend note",
            "",
            &Theme::default(),
            80,
        )
        .into_iter()
        .flat_map(|line| line.spans)
        .map(|span| span.content.into_owned())
        .collect::<Vec<_>>()
        .join("\n");

        // assert
        assert!(
            text.contains("Waiting for input") && text.contains("Done"),
            "{text}"
        );
        assert!(
            !text.contains("internal detail") && !text.contains(" as W"),
            "{text}"
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
            spans
                .iter()
                .any(|span| span.style.fg == Some(theme.markdown.rule)),
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
        let (lines, _, _) =
            try_render_markdown_table_block(&rows, theme.text.primary, "", &theme, 80).unwrap();
        let spans = collect_spans(&lines);
        assert!(
            spans.iter().any(|span| {
                span.style.fg == Some(theme.markdown.heading_h1)
                    && span.style.add_modifier.contains(Modifier::BOLD)
            }),
            "table header should use theme.markdown.heading with BOLD, got: {spans:?}"
        );
    }
}
