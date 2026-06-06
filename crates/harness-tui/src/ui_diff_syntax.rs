use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors,
    StyleModifier as SyntectStyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings as SyntectThemeSettings,
};
use syntect::parsing::SyntaxSet;

use super::super::ui_chrome::{display_width, take_width_prefix, truncate_plain_text};

#[derive(Debug, Clone)]
pub(crate) struct StyledTextChunk {
    pub(crate) text: String,
    pub(crate) style: Style,
}

pub(super) fn styled_chunks_to_spans(chunks: Vec<StyledTextChunk>) -> Vec<Span<'static>> {
    chunks
        .into_iter()
        .map(|chunk| Span::styled(chunk.text, chunk.style))
        .collect()
}

pub(super) fn truncate_styled_chunks(
    chunks: &[StyledTextChunk],
    max_width: usize,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut rendered = Vec::new();
    let mut used = 0usize;
    for chunk in chunks {
        if used >= max_width {
            break;
        }
        let remaining = max_width - used;
        let text = truncate_plain_text(&chunk.text, remaining);
        used += display_width(&text);
        rendered.push(Span::styled(text, chunk.style));
    }
    rendered
}

pub(super) fn wrap_styled_chunks(
    chunks: &[StyledTextChunk],
    max_width: usize,
) -> Vec<Vec<StyledTextChunk>> {
    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut lines = vec![Vec::new()];
    let mut remaining = max_width;

    for chunk in chunks {
        let mut rest = chunk.text.as_str();
        if rest.is_empty() {
            continue;
        }

        loop {
            if remaining == 0 {
                lines.push(Vec::new());
                remaining = max_width;
            }

            let piece = take_width_prefix(rest, remaining);
            if piece.is_empty() {
                lines.push(Vec::new());
                remaining = max_width;
                continue;
            }

            if let Some(current) = lines.last_mut() {
                current.push(StyledTextChunk {
                    text: piece.to_string(),
                    style: chunk.style,
                });
            }
            remaining = remaining.saturating_sub(display_width(piece));
            rest = &rest[piece.len()..];

            if rest.is_empty() {
                break;
            }

            lines.push(Vec::new());
            remaining = max_width;
        }
    }

    lines
}

pub(super) fn highlight_diff_line_chunks(
    path: Option<&str>,
    text: &str,
    row_bg: Option<Color>,
) -> Option<Vec<StyledTextChunk>> {
    let path = path?;
    if diff_path_is_plain_prose(path) {
        return None;
    }
    let assets = diff_syntax_highlight_assets();
    let syntax = assets
        .syntax_set
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| {
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str())
                .and_then(|extension| assets.syntax_set.find_syntax_by_extension(extension))
        })?;
    let mut highlighter = HighlightLines::new(syntax, &assets.theme);
    let regions = highlighter.highlight_line(text, &assets.syntax_set).ok()?;
    Some(
        regions
            .into_iter()
            .map(|(style, content)| StyledTextChunk {
                text: content.to_string(),
                style: diff_syntect_style_to_ratatui(style, row_bg),
            })
            .collect(),
    )
}

pub(super) fn diff_path_is_plain_prose(path: &str) -> bool {
    let Some(extension) = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
    else {
        return false;
    };

    matches!(
        extension.to_ascii_lowercase().as_str(),
        "adoc" | "asciidoc" | "markdown" | "md" | "mdown" | "mkd" | "rst" | "text" | "txt"
    )
}

fn diff_syntax_highlight_assets() -> &'static DiffSyntaxHighlightAssets {
    static SYNTAX_ASSETS: OnceLock<DiffSyntaxHighlightAssets> = OnceLock::new();

    SYNTAX_ASSETS.get_or_init(|| {
        let syntax_set = SyntaxSet::load_defaults_nonewlines();
        let theme = reference_diff_syntect_theme();
        DiffSyntaxHighlightAssets { syntax_set, theme }
    })
}

fn reference_diff_syntect_theme() -> SyntectTheme {
    let mut scopes = Vec::new();
    push_syntect_scope(
        &mut scopes,
        "comment, comment.documentation",
        Some(reference_syntax_comment()),
        None,
        Some(SyntectFontStyle::ITALIC),
    );
    push_syntect_scope(
        &mut scopes,
        "string, string.quoted, string.unquoted, symbol, character.special, constant.character.escape",
        Some(reference_syntax_string()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "number, boolean, constant.numeric, constant.language.boolean, constant",
        Some(reference_syntax_number()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "keyword, keyword.control, keyword.return, keyword.conditional, keyword.repeat, keyword.coroutine, storage, storage.modifier",
        Some(reference_syntax_keyword()),
        None,
        Some(SyntectFontStyle::ITALIC),
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.import, keyword.export, string.escape, string.regexp, keyword.directive, keyword.modifier, keyword.exception, tag.attribute",
        Some(reference_syntax_keyword()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.type, storage.type, storage.type.primitive",
        Some(reference_syntax_type()),
        None,
        Some(SyntectFontStyle::BOLD.union(SyntectFontStyle::ITALIC)),
    );
    push_syntect_scope(
        &mut scopes,
        "keyword.function, function.method, variable.member, function, constructor, entity.name.function, support.function, support.function.builtin",
        Some(reference_syntax_function()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "variable, variable.parameter, function.method.call, function.call, property, parameter, field",
        Some(reference_syntax_variable()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "type, module, namespace, class, type.definition, entity.name.type, support.type, support.class",
        Some(reference_syntax_type()),
        None,
        Some(SyntectFontStyle::BOLD),
    );
    push_syntect_scope(
        &mut scopes,
        "operator, keyword.operator, keyword.operator.word, punctuation.delimiter, punctuation.separator, keyword.conditional.ternary, tag.delimiter",
        Some(reference_syntax_operator()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "punctuation, punctuation.bracket",
        Some(reference_syntax_punctuation()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "variable.builtin, type.builtin, function.builtin, module.builtin, constant.builtin, tag, attribute, annotation",
        Some(reference_syntax_error()),
        None,
        None,
    );
    push_syntect_scope(
        &mut scopes,
        "markup.raw, markup.raw.block, markup.raw.inline",
        Some(reference_syntax_string()),
        None,
        None,
    );

    SyntectTheme {
        name: Some("harness-diff".to_string()),
        author: Some("agent-harness".to_string()),
        settings: SyntectThemeSettings {
            foreground: Some(reference_syntax_punctuation()),
            background: Some(reference_diff_context_bg()),
            ..SyntectThemeSettings::default()
        },
        scopes,
    }
}

fn push_syntect_scope(
    scopes: &mut Vec<ThemeItem>,
    selector: &str,
    foreground: Option<SyntectColor>,
    background: Option<SyntectColor>,
    font_style: Option<SyntectFontStyle>,
) {
    let scope = ScopeSelectors::from_str(selector)
        .unwrap_or_else(|error| panic!("invalid syntect selector {selector:?}: {error:?}"));
    scopes.push(ThemeItem {
        scope,
        style: SyntectStyleModifier {
            foreground,
            background,
            font_style,
        },
    });
}

fn syntect_rgb(red: u8, green: u8, blue: u8) -> SyntectColor {
    SyntectColor {
        r: red,
        g: green,
        b: blue,
        a: 0xFF,
    }
}

fn reference_diff_context_bg() -> SyntectColor {
    syntect_rgb(0x14, 0x14, 0x14)
}

fn reference_syntax_comment() -> SyntectColor {
    syntect_rgb(0x80, 0x80, 0x80)
}

fn reference_syntax_keyword() -> SyntectColor {
    syntect_rgb(0x9D, 0x7C, 0xD8)
}

fn reference_syntax_function() -> SyntectColor {
    syntect_rgb(0xFA, 0xB2, 0x83)
}

fn reference_syntax_variable() -> SyntectColor {
    syntect_rgb(0xE0, 0x6C, 0x75)
}

fn reference_syntax_string() -> SyntectColor {
    syntect_rgb(0x7F, 0xD8, 0x8F)
}

fn reference_syntax_number() -> SyntectColor {
    syntect_rgb(0xF5, 0xA7, 0x42)
}

fn reference_syntax_type() -> SyntectColor {
    syntect_rgb(0xE5, 0xC0, 0x7B)
}

fn reference_syntax_operator() -> SyntectColor {
    syntect_rgb(0x56, 0xB6, 0xC2)
}

fn reference_syntax_punctuation() -> SyntectColor {
    syntect_rgb(0xEE, 0xEE, 0xEE)
}

fn reference_syntax_error() -> SyntectColor {
    syntect_rgb(0xE0, 0x6C, 0x75)
}

struct DiffSyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
}

fn diff_syntect_style_to_ratatui(
    style: syntect::highlighting::Style,
    row_bg: Option<Color>,
) -> Style {
    let mut rendered = Style::default().fg(Color::Rgb(
        style.foreground.r,
        style.foreground.g,
        style.foreground.b,
    ));

    if let Some(row_bg) = row_bg {
        rendered = rendered.bg(row_bg);
    }
    if style.font_style.contains(SyntectFontStyle::BOLD) {
        rendered = rendered.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(SyntectFontStyle::ITALIC) {
        rendered = rendered.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(SyntectFontStyle::UNDERLINE) {
        rendered = rendered.add_modifier(Modifier::UNDERLINED);
    }
    rendered
}
