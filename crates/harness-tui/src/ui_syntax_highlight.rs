use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
#[path = "ui_syntax_highlight/cache.rs"]
mod cache;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme};
use syntect::parsing::SyntaxSet;

use crate::theme::{quantize_color, Theme};

struct SyntaxHighlightAssets {
    syntax_set: SyntaxSet,
}

pub(super) fn render_highlighted_code_block(
    language: Option<&str>,
    body: &str,
    _raw: &str,
    _prefix: &str,
    color: ratatui::style::Color,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    let highlighted = language.and_then(|language| {
        let syntax_assets = syntax_highlight_assets();
        let syntax = syntax_assets
            .syntax_set
            .find_syntax_by_token(language)
            .or_else(|| {
                let token = language.rsplit(':').next().unwrap_or(language);
                std::path::Path::new(token)
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .and_then(|ext| syntax_assets.syntax_set.find_syntax_by_extension(ext))
            })?;
        let palette = syntax_theme(theme)?;
        let highlighted = cache::highlight(
            syntax,
            &syntax_assets.syntax_set,
            palette,
            theme.is_dark(),
            body,
        )?;
        Some(
            highlighted
                .into_iter()
                .map(|regions| {
                    Line::from(
                        regions
                            .into_iter()
                            .map(|(style, content)| {
                                Span::styled(content, syntect_style_to_ratatui(style, theme))
                            })
                            .collect::<Vec<_>>(),
                    )
                })
                .collect::<Vec<_>>(),
        )
    });

    if let Some(highlighted) = highlighted {
        lines.extend(highlighted);
    } else {
        for source_line in body.lines() {
            lines.push(Line::from(Span::styled(
                source_line.to_string(),
                Style::default()
                    .fg(color)
                    .bg(theme.markdown.code_background),
            )));
        }
    }

    if body.is_empty() {
        lines.push(Line::from(Span::styled(
            "",
            Style::default()
                .fg(color)
                .bg(theme.markdown.code_background),
        )));
    }

    lines
}

fn syntax_highlight_assets() -> &'static SyntaxHighlightAssets {
    static SYNTAX_ASSETS: OnceLock<SyntaxHighlightAssets> = OnceLock::new();

    SYNTAX_ASSETS.get_or_init(|| {
        let syntax_set = two_face::syntax::extra_newlines();
        SyntaxHighlightAssets { syntax_set }
    })
}

fn syntax_theme(theme: &Theme) -> Option<&'static SyntectTheme> {
    static DARK: OnceLock<Option<SyntectTheme>> = OnceLock::new();
    static LIGHT: OnceLock<Option<SyntectTheme>> = OnceLock::new();
    let (cache, bytes): (_, &[u8]) = if theme.is_dark() {
        (&DARK, include_bytes!("../assets/syntax/grok-night.tmTheme"))
    } else {
        (&LIGHT, include_bytes!("../assets/syntax/grok-day.tmTheme"))
    };
    cache
        .get_or_init(|| {
            syntect::highlighting::ThemeSet::load_from_reader(&mut std::io::Cursor::new(bytes)).ok()
        })
        .as_ref()
}

fn syntect_style_to_ratatui(style: syntect::highlighting::Style, theme: &Theme) -> Style {
    let foreground = if theme.text.primary == Color::Reset {
        native_syntax_foreground(style.foreground)
    } else {
        syntect_color_to_ratatui(style.foreground)
    };
    let mut rendered = Style::default()
        .fg(quantize_color(foreground, theme.color_level()))
        .bg(theme.markdown.code_background);

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

fn syntect_color_to_ratatui(color: syntect::highlighting::Color) -> ratatui::style::Color {
    Color::Rgb(color.r, color.g, color.b)
}

// Adapted from Grok Build's pager-render/src/syntax.rs (Apache-2.0; see assets/syntax).
// Default foreground and base ANSI hues retain contrast on either host polarity.
fn native_syntax_foreground(color: syntect::highlighting::Color) -> Color {
    let (r, g, b) = (i32::from(color.r), i32::from(color.g), i32::from(color.b));
    let max = r.max(g).max(b);
    let chroma = max - r.min(g).min(b);
    if chroma < 40 {
        return Color::Reset;
    }
    let hue = if max == r {
        ((g - b) * 60 / chroma).rem_euclid(360)
    } else if max == g {
        (b - r) * 60 / chroma + 120
    } else {
        (r - g) * 60 / chroma + 240
    };
    match hue {
        0..30 | 330..=360 => Color::Red,
        30..90 => Color::Yellow,
        90..150 => Color::Green,
        150..210 => Color::Cyan,
        210..255 => Color::Blue,
        _ => Color::Magenta,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::render_highlighted_code_block;

    #[test]
    fn known_language_uses_non_default_highlight_styles() {
        let lines = render_highlighted_code_block(
            Some("rust"),
            "fn main() {",
            "```rust\nfn main() {\n```",
            "",
            Color::Gray,
            &crate::theme::Theme::default(),
        );

        assert_eq!(lines.len(), 1);
        assert!(
            lines[0]
                .spans
                .iter()
                .any(|span| span.style.fg.is_some_and(|color| color != Color::Gray)),
            "known-language code should use syntax colors: {:?}",
            lines[0]
        );
    }

    #[test]
    fn unknown_language_falls_back_to_plain_color() {
        let lines = render_highlighted_code_block(
            Some("not-a-language"),
            "plain text",
            "```not-a-language\nplain text\n```",
            "",
            Color::Blue,
            &crate::theme::Theme::default(),
        );

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].content.as_ref(), "plain text");
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Blue));
    }
}
