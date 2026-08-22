use std::sync::OnceLock;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Theme as SyntectTheme};
use syntect::parsing::SyntaxSet;

use crate::theme::{quantize_color, Theme};

struct SyntaxHighlightAssets {
    syntax_set: SyntaxSet,
    theme: SyntectTheme,
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
        let syntax = syntax_assets.syntax_set.find_syntax_by_token(language)?;
        let mut highlighter = HighlightLines::new(syntax, &syntax_assets.theme);
        let mut lines = Vec::new();
        for source_line in body.lines() {
            let Ok(regions) = highlighter.highlight_line(source_line, &syntax_assets.syntax_set)
            else {
                return None;
            };
            if regions.is_empty() {
                lines.push(Line::from(Span::styled(
                    source_line.to_string(),
                    Style::default()
                        .fg(color)
                        .bg(theme.markdown.code_background),
                )));
            } else {
                lines.push(Line::from(
                    regions
                        .into_iter()
                        .map(|(style, content)| {
                            Span::styled(
                                content.to_string(),
                                syntect_style_to_ratatui(style, theme),
                            )
                        })
                        .collect::<Vec<_>>(),
                ));
            }
        }
        Some(lines)
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
        let syntax_set = SyntaxSet::load_defaults_nonewlines();
        let theme = super::ui_diff::ui_diff_syntax::diff_syntect_theme();
        SyntaxHighlightAssets { syntax_set, theme }
    })
}

fn syntect_style_to_ratatui(style: syntect::highlighting::Style, theme: &Theme) -> Style {
    let foreground = syntect_color_to_ratatui(style.foreground);
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
        // arrange
        // act
        // assert
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
