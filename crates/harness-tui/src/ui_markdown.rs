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

pub(super) fn parse_inline_markdown_spans(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if let Some(rest) = remaining.strip_prefix('[') {
            if let Some(label_end) = rest.find("](") {
                let after_label = &rest[label_end + 2..];
                if let Some(url_end) = after_label.find(')') {
                    spans.push(Span::styled(
                        rest[..label_end].to_string(),
                        base_style
                            .fg(theme.text.accent)
                            .add_modifier(Modifier::UNDERLINED),
                    ));
                    remaining = &after_label[url_end + 1..];
                    continue;
                }
            }
        }

        if let Some(url_end) = raw_url_length(remaining) {
            spans.push(Span::styled(
                remaining[..url_end].to_string(),
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::UNDERLINED),
            ));
            remaining = &remaining[url_end..];
            continue;
        }

        if let Some(rest) = remaining.strip_prefix("**") {
            if let Some(end) = rest.find("**") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.fg(base_color).add_modifier(Modifier::BOLD),
                ));
                remaining = &rest[end + 2..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix("~~") {
            if let Some(end) = rest.find("~~") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.text.secondary)
                        .add_modifier(Modifier::CROSSED_OUT),
                ));
                remaining = &rest[end + 2..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('`') {
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style.fg(theme.status.success),
                ));
                remaining = &rest[end + 1..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('*') {
            if let Some(end) = rest.find('*') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.status.warning)
                        .add_modifier(Modifier::ITALIC),
                ));
                remaining = &rest[end + 1..];
                continue;
            }
        }

        if let Some(rest) = remaining.strip_prefix('_') {
            if let Some(end) = rest.find('_') {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    base_style
                        .fg(theme.status.warning)
                        .add_modifier(Modifier::ITALIC),
                ));
                remaining = &rest[end + 1..];
                continue;
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
            remaining = &remaining[ch.len_utf8()..];
            continue;
        }
        let plain = &remaining[..next_marker];
        spans.push(Span::styled(plain.to_string(), base_style.fg(base_color)));
        remaining = &remaining[next_marker..];
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
                    .fg(theme.text.accent)
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
                    .fg(theme.text.accent)
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
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}"),
            base_style,
            parse_inline_markdown_spans(
                text,
                base_style
                    .fg(theme.text.accent)
                    .add_modifier(Modifier::BOLD),
                theme.text.accent,
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
                Style::default().fg(theme.text.secondary),
            )],
            width,
        );
        return;
    }

    if let Some(text) = trimmed.strip_prefix("> ") {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}▍ "),
            Style::default().fg(theme.text.secondary),
            parse_inline_markdown_spans(
                text,
                Style::default()
                    .fg(theme.text.secondary)
                    .add_modifier(Modifier::ITALIC),
                theme.text.secondary,
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

fn raw_url_length(text: &str) -> Option<usize> {
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
