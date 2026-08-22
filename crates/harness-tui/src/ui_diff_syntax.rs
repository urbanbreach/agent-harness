// allow: SIZE_OK — TUI diff rendering (indivisible view model)
use std::path::Path;
use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::Span,
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme, ThemeSet};
use syntect::parsing::SyntaxSet;

use crate::theme::{quantize_color, ColorLevel};

use super::super::ui_chrome::{display_width, take_width_prefix};

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
    color_level: ColorLevel,
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
                style: diff_syntect_style_to_ratatui(style, row_bg, color_level),
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
        let theme = diff_syntect_theme();
        DiffSyntaxHighlightAssets { syntax_set, theme }
    })
}

pub(crate) fn diff_syntect_theme() -> SyntectTheme {
    ThemeSet::load_defaults()
        .themes
        .remove("base16-ocean.dark")
        .unwrap_or_default()
}

struct DiffSyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
}

fn diff_syntect_style_to_ratatui(
    style: syntect::highlighting::Style,
    row_bg: Option<Color>,
    color_level: ColorLevel,
) -> Style {
    let foreground = Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b);
    let mut rendered = Style::default().fg(quantize_color(foreground, color_level));

    if let Some(row_bg) = row_bg {
        rendered = rendered.bg(quantize_color(row_bg, color_level));
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
