// allow: SIZE_OK — TUI rendering (indivisible view model)
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use std::borrow::Cow;

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InlineMarkdownLink {
    pub(super) label: String,
    pub(super) start_cell: usize,
    pub(super) end_cell: usize,
    pub(super) destination: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct ParsedInlineMarkdown {
    pub(super) spans: Vec<Span<'static>>,
    pub(super) links: Vec<InlineMarkdownLink>,
}

impl ParsedInlineMarkdown {
    fn push(&mut self, text: &str, style: Style, destination: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let start_cell = self.spans.iter().map(Span::width).sum::<usize>();
        let end_cell = start_cell.saturating_add(display_width(text));
        if let Some(destination) = destination.filter(|destination| {
            crate::transcript_selection::Hyperlink::new(
                text,
                destination,
                crate::transcript_selection::LinkRange::new(
                    0,
                    start_cell,
                    end_cell.saturating_sub(1),
                ),
            )
            .is_ok()
        }) {
            if let Some(previous) = self.links.last_mut().filter(|previous| {
                previous.end_cell == start_cell && previous.destination == destination
            }) {
                previous.label.push_str(text);
                previous.end_cell = end_cell;
            } else {
                self.links.push(InlineMarkdownLink {
                    label: text.to_string(),
                    start_cell,
                    end_cell,
                    destination: destination.to_string(),
                });
            }
        }
        if let Some(previous) = self
            .spans
            .last_mut()
            .filter(|previous| previous.style == style)
        {
            previous.content.to_mut().push_str(text);
        } else {
            self.spans.push(Span::styled(text.to_string(), style));
        }
    }

    fn push_text(&mut self, mut text: &str, style: Style, theme: &Theme) {
        while !text.is_empty() {
            let next_url = ["https://", "http://"]
                .into_iter()
                .filter_map(|prefix| text.find(prefix))
                .min();
            let Some(start) = next_url else {
                self.push(text, style, None);
                break;
            };
            self.push(&text[..start], style, None);
            text = &text[start..];
            let length = raw_url_length(text).unwrap_or(text.len());
            let destination = &text[..length];
            self.push(
                destination,
                style
                    .fg(theme.markdown.link)
                    .add_modifier(Modifier::UNDERLINED),
                Some(destination),
            );
            text = &text[length..];
        }
    }
}

pub(super) fn parse_inline_markdown(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> ParsedInlineMarkdown {
    // Inline callers include table cells. The sentinel prevents a leading "#",
    // "-", or fence marker in a cell from becoming a block construct.
    let normalized = normalize_math_delimiters(text);
    let source = format!(".{normalized}");
    let options = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_MATH;
    let mut parsed = ParsedInlineMarkdown::default();
    let mut style = base_style.fg(base_color);
    let mut destination = None;
    let mut ancestors = Vec::new();

    for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
        // Match Grok's double-tilde-only strikethrough contract.
        if matches!(
            event,
            Event::Start(Tag::Strikethrough) | Event::End(TagEnd::Strikethrough)
        ) && !source[range.clone()].starts_with("~~")
        {
            parsed.push("~", style, destination.as_deref());
            continue;
        }
        match event {
            Event::Start(tag) => {
                ancestors.push((style, destination.clone()));
                match tag {
                    Tag::Strong => {
                        style = style.add_modifier(Modifier::BOLD);
                        if destination.is_none() {
                            style = style.fg(theme.markdown.strong);
                        }
                    }
                    Tag::Emphasis => {
                        style = style.add_modifier(Modifier::ITALIC);
                        if destination.is_none() {
                            style = style.fg(theme.markdown.emph);
                        }
                    }
                    Tag::Strikethrough => {
                        style = style.add_modifier(Modifier::CROSSED_OUT);
                        if destination.is_none() {
                            style = style.fg(theme.text.secondary);
                        }
                    }
                    Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                        destination = Some(dest_url);
                        style = style
                            .fg(theme.markdown.link_text)
                            .add_modifier(Modifier::UNDERLINED);
                    }
                    _ => {}
                }
            }
            Event::End(_) => {
                if let Some(parent) = ancestors.pop() {
                    (style, destination) = parent;
                }
            }
            Event::Text(value) => {
                let value = if range.start == 0 {
                    value.strip_prefix('.').unwrap_or(&value)
                } else {
                    &value
                };
                if destination.is_some() {
                    parsed.push(value, style, destination.as_deref());
                } else {
                    parsed.push_text(value, style, theme);
                }
            }
            Event::Code(value) => parsed.push(
                &value,
                style.fg(theme.markdown.code).add_modifier(Modifier::BOLD),
                destination.as_deref(),
            ),
            Event::InlineMath(math) | Event::DisplayMath(math) => {
                if let Some(rendered) = terminal_math(&math) {
                    let math_style = if destination.is_some() {
                        style
                    } else {
                        style.fg(theme.markdown.code)
                    };
                    parsed.push(
                        &rendered,
                        math_style.add_modifier(Modifier::ITALIC),
                        destination.as_deref(),
                    );
                } else {
                    parsed.push(&source[range], style, destination.as_deref());
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                parsed.push(" ", style, destination.as_deref());
            }
            Event::Html(value) | Event::InlineHtml(value) => {
                parsed.push(&value, style, destination.as_deref());
            }
            _ => {}
        }
    }
    parsed
}

pub(super) fn parse_inline_markdown_spans(
    text: &str,
    base_style: Style,
    base_color: Color,
    theme: &Theme,
) -> Vec<Span<'static>> {
    parse_inline_markdown(text, base_style, base_color, theme).spans
}

pub(super) fn markdown_heading_text(line: &str) -> Option<&str> {
    markdown_heading(line).map(|(_, text)| text)
}

const MAX_MATH_SOURCE_BYTES: usize = 4096;

fn math_delimiter_end(text: &str, closing: &str) -> Option<usize> {
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if index > MAX_MATH_SOURCE_BYTES {
            return None;
        }
        if !escaped && text[index..].starts_with(closing) {
            return Some(index);
        }
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        }
    }
    None
}

fn normalize_math_delimiters(text: &str) -> Cow<'_, str> {
    if !text.contains("\\(") && !text.contains("\\[") && !text.contains("$$") {
        return Cow::Borrowed(text);
    }
    // Code stays literal. Protect complete links too: destinations must never
    // be rewritten as presentation math.
    let protected = Parser::new_ext(text, Options::ENABLE_MATH)
        .into_offset_iter()
        .filter_map(|(event, range)| {
            matches!(
                event,
                Event::Code(_)
                    | Event::Start(Tag::CodeBlock(_) | Tag::Link { .. } | Tag::Image { .. })
            )
            .then_some(range)
        })
        .collect::<Vec<_>>();
    let mut protected_index = 0;
    let mut cursor = 0;
    let mut result = String::with_capacity(text.len());
    while cursor < text.len() {
        while protected
            .get(protected_index)
            .is_some_and(|range| range.end <= cursor)
        {
            protected_index += 1;
        }
        if let Some(range) = protected
            .get(protected_index)
            .filter(|range| range.start == cursor)
        {
            result.push_str(&text[range.clone()]);
            cursor = range.end;
            continue;
        }
        let remaining = &text[cursor..];
        if remaining.starts_with("\\\\") || remaining.starts_with("\\$") {
            result.push_str(&remaining[..2]);
            cursor += 2;
            continue;
        }
        let delimiters = if remaining.starts_with("\\(") {
            Some(("\\)", "$"))
        } else if remaining.starts_with("\\[") {
            Some(("\\]", "$$"))
        } else if remaining.starts_with("$$") {
            Some(("$$", "$$"))
        } else {
            None
        };
        if let Some((closing, canonical)) = delimiters {
            if let Some(end) = math_delimiter_end(&remaining[2..], closing) {
                let body = remaining[2..2 + end].trim();
                let crosses_block = body.lines().any(|line| line.trim().is_empty())
                    || body
                        .lines()
                        .skip(1)
                        .any(|line| line.trim_start().starts_with(['>', '|']));
                if !body.is_empty() && !crosses_block {
                    result.push_str(canonical);
                    result.push_str(&body.lines().map(str::trim).collect::<Vec<_>>().join(" "));
                    result.push_str(canonical);
                    cursor += 2 + end + closing.len();
                    continue;
                }
            }
        }
        if let Some(ch) = remaining.chars().next() {
            result.push(ch);
            cursor += ch.len_utf8();
        }
    }
    Cow::Owned(result)
}

/// Paint and selection share this projection; original transcript data remains
/// unchanged for source views and source-oriented copy.
pub(super) fn markdown_display_source(text: &str) -> String {
    let source = normalize_math_delimiters(text);
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_MATH;
    let mut replacements = Vec::new();
    let mut quote_ranges = Vec::new();
    let mut joined_line_starts = Vec::new();
    for (event, range) in Parser::new_ext(&source, options).into_offset_iter() {
        match event {
            Event::SoftBreak => {
                // Match Grok: explicit container/indented continuations stay
                // on their own visual lines.
                if !matches!(
                    source.as_bytes().get(range.end),
                    Some(b' ' | b'\t' | b'>' | b'|')
                ) {
                    joined_line_starts.push(range.end);
                    replacements.push((range, " ".to_string()));
                }
            }
            Event::HardBreak => replacements.push((range, "\n".to_string())),
            Event::Start(Tag::BlockQuote(_)) => quote_ranges.push(range),
            _ => {}
        }
    }
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let row = line.trim_end_matches(['\r', '\n']);
        let trimmed = row.trim_start();
        let (_, body) = markdown_quote_prefix(trimmed).unwrap_or((0, trimmed));
        let body_start = offset + row.len() - body.len();
        // Parser ranges retain inherited depth on lazy continuation lines.
        let depth = quote_ranges
            .iter()
            .filter(|range| range.contains(&body_start))
            .count();
        if depth > 0 && !joined_line_starts.contains(&offset) {
            let indent = &row[..row.len() - trimmed.len()];
            replacements.push((
                offset..body_start,
                format!("{indent}{}", "> ".repeat(depth)),
            ));
        }
        offset += line.len();
    }
    replacements.sort_by_key(|(range, _)| range.start);
    let mut rendered = source.into_owned();
    for (range, replacement) in replacements.into_iter().rev() {
        rendered.replace_range(range, &replacement);
    }
    rendered
}

pub(super) fn markdown_quote_prefix(mut text: &str) -> Option<(usize, &str)> {
    let mut depth = 0;
    while let Some(body) = text.trim_start().strip_prefix('>') {
        depth += 1;
        text = body.strip_prefix(' ').unwrap_or(body);
    }
    (depth > 0).then_some((depth, text))
}

fn terminal_math(source: &str) -> Option<String> {
    if source.len() > MAX_MATH_SOURCE_BYTES {
        return None;
    }
    let mut remaining = source;
    let rendered = math_sequence(&mut remaining, 0, false)?;
    (!rendered.trim().is_empty()).then(|| rendered.trim().to_string())
}

fn math_sequence(source: &mut &str, depth: usize, grouped: bool) -> Option<String> {
    if depth >= 32 {
        return None;
    }
    let mut result = String::new();
    while let Some(ch) = source.chars().next() {
        match ch {
            '}' => {
                if !grouped {
                    return None;
                }
                *source = &source[1..];
                return Some(result);
            }
            '^' | '_' => {
                *source = source[1..].trim_start();
                let atom = math_atom(source, depth)?;
                let (plain, script) = if ch == '^' {
                    ("0123456789+-=()ni", "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾ⁿⁱ")
                } else {
                    (
                        "0123456789+-=()aehijklmnoprstuvx",
                        "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎ₐₑₕᵢⱼₖₗₘₙₒₚᵣₛₜᵤᵥₓ",
                    )
                };
                let mapped = atom
                    .chars()
                    .map(|letter| {
                        plain
                            .chars()
                            .position(|candidate| candidate == letter)
                            .and_then(|index| script.chars().nth(index))
                    })
                    .collect::<Option<String>>();
                if let Some(mapped) = mapped.filter(|value| !value.is_empty()) {
                    result.push_str(&mapped);
                } else if atom.chars().count() > 1 {
                    result.push_str(&format!("{ch}({atom})"));
                } else {
                    result.push(ch);
                    result.push_str(&atom);
                }
            }
            ch if ch.is_whitespace() => {
                *source = &source[ch.len_utf8()..];
                if !result.is_empty() && !result.ends_with(' ') {
                    result.push(' ');
                }
            }
            _ => result.push_str(&math_atom(source, depth)?),
        }
    }
    (!grouped).then_some(result)
}

fn math_atom(source: &mut &str, depth: usize) -> Option<String> {
    if depth >= 32 {
        return None;
    }
    let ch = source.chars().next()?;
    *source = &source[ch.len_utf8()..];
    match ch {
        '{' => math_sequence(source, depth + 1, true).map(|value| value.trim().to_string()),
        '}' | '^' | '_' => None,
        '-' => Some("−".to_string()),
        '\'' => Some("′".to_string()),
        '\\' => {
            let remaining = *source;
            let length = remaining
                .bytes()
                .take_while(u8::is_ascii_alphabetic)
                .count();
            let command = &remaining[..length];
            *source = &remaining[length..];
            match command {
                "frac" | "dfrac" | "tfrac" => {
                    *source = source.trim_start();
                    let numerator = math_atom(source, depth + 1)?;
                    *source = source.trim_start();
                    let denominator = math_atom(source, depth + 1)?;
                    if numerator.is_empty() || denominator.is_empty() {
                        return None;
                    }
                    let fraction = match (numerator.as_str(), denominator.as_str()) {
                        ("1", "2") => Some("½"),
                        ("1", "3") => Some("⅓"),
                        ("2", "3") => Some("⅔"),
                        ("1", "4") => Some("¼"),
                        ("3", "4") => Some("¾"),
                        ("1", "5") => Some("⅕"),
                        ("2", "5") => Some("⅖"),
                        ("3", "5") => Some("⅗"),
                        ("4", "5") => Some("⅘"),
                        ("1", "6") => Some("⅙"),
                        ("5", "6") => Some("⅚"),
                        ("1", "8") => Some("⅛"),
                        ("3", "8") => Some("⅜"),
                        ("5", "8") => Some("⅝"),
                        ("7", "8") => Some("⅞"),
                        _ => None,
                    };
                    let parenthesize = |value: String| {
                        if value.chars().count() > 1 {
                            format!("({value})")
                        } else {
                            value
                        }
                    };
                    Some(fraction.map_or_else(
                        || format!("{}/{}", parenthesize(numerator), parenthesize(denominator)),
                        str::to_string,
                    ))
                }
                "sqrt" => {
                    *source = source.trim_start();
                    if source.starts_with('[') {
                        return None;
                    }
                    let atom = math_atom(source, depth + 1)?;
                    Some(if atom.chars().count() > 1 {
                        format!("√({atom})")
                    } else {
                        format!("√{atom}")
                    })
                }
                "" => {
                    let escaped = source.chars().next()?;
                    *source = &source[escaped.len_utf8()..];
                    match escaped {
                        '\\' => Some("; ".to_string()),
                        ',' | ';' | ':' | ' ' => Some(" ".to_string()),
                        '!' => Some(String::new()),
                        '{' | '}' | '_' | '%' | '$' | '#' | '&' | '|' => Some(escaped.to_string()),
                        _ => None,
                    }
                }
                _ => Some(
                    match command {
                        "alpha" => "α",
                        "beta" => "β",
                        "gamma" => "γ",
                        "delta" => "δ",
                        "epsilon" => "ε",
                        "theta" => "θ",
                        "lambda" => "λ",
                        "mu" => "μ",
                        "pi" => "π",
                        "rho" => "ρ",
                        "sigma" => "σ",
                        "tau" => "τ",
                        "phi" => "φ",
                        "chi" => "χ",
                        "psi" => "ψ",
                        "omega" => "ω",
                        "Gamma" => "Γ",
                        "Delta" => "Δ",
                        "Theta" => "Θ",
                        "Lambda" => "Λ",
                        "Pi" => "Π",
                        "Sigma" => "Σ",
                        "Phi" => "Φ",
                        "Psi" => "Ψ",
                        "Omega" => "Ω",
                        "le" | "leq" => "≤",
                        "ge" | "geq" => "≥",
                        "ne" | "neq" => "≠",
                        "times" => "×",
                        "cdot" => "·",
                        "pm" => "±",
                        "infty" => "∞",
                        "sum" => "∑",
                        "prod" => "∏",
                        "int" => "∫",
                        "partial" => "∂",
                        "nabla" => "∇",
                        "approx" => "≈",
                        "to" | "rightarrow" => "→",
                        "in" => "∈",
                        "notin" => "∉",
                        "forall" => "∀",
                        "exists" => "∃",
                        _ => return None,
                    }
                    .to_string(),
                ),
            }
        }
        ch if ch.is_control() => None,
        ch => Some(ch.to_string()),
    }
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
    if !text.contains("```") && !text.contains("~~~") {
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
                if matches!(language.as_deref(), Some("diff" | "patch")) {
                    if let Some(diff_lines) =
                        render_structured_diff_lines(&body, None, prefix, width, false, theme)
                    {
                        lines.extend(diff_lines);
                        continue;
                    }
                }

                if is_mermaid_language(language.as_deref()) {
                    if !lines.is_empty() && !lines.last().is_some_and(|line| line.spans.is_empty())
                    {
                        lines.push(Line::default());
                    }
                    lines.extend(render_mermaid_diagram(&body, prefix, theme, width));
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
    let display_source = markdown_display_source(text);
    let rows = display_source.lines().collect::<Vec<_>>();
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
        if !last_line_is_visually_blank(lines) {
            append_prefixed_wrapped_spans_line(lines, prefix, base_style, Vec::new(), width);
        }
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

    if let Some((depth, text)) = markdown_quote_prefix(trimmed) {
        append_prefixed_wrapped_spans_line(
            lines,
            &format!("{prefix}{indent}{}", "│ ".repeat(depth)),
            Style::default().fg(theme.markdown.block_quote),
            parse_inline_markdown_spans(
                text,
                Style::default().fg(theme.markdown.block_quote),
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
    fn intraword_asterisks_follow_commonmark_emphasis() {
        let theme = Theme::default();
        let base = Style::default().fg(theme.text.primary);
        let spans = parse_inline_markdown_spans("foo*bar*baz", base, theme.text.primary, &theme);
        let emphasized = spans
            .iter()
            .find(|span| span.content.as_ref() == "bar")
            .expect("CommonMark emphasis body");
        assert!(emphasized.style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(emphasized.style.fg, Some(theme.markdown.emph));
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
                    && !span.style.add_modifier.contains(Modifier::ITALIC)
            }),
            "blockquote should use its theme role without implicit italic, got: {spans:?}"
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
