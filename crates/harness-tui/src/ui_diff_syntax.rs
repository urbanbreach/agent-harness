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
use unicode_segmentation::UnicodeSegmentation;

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
    let mut source_column = 0;

    for chunk in chunks {
        let mut expanded = String::new();
        for grapheme in chunk.text.graphemes(true) {
            if grapheme == "\t" {
                let spaces = 4 - source_column % 4;
                expanded.extend(std::iter::repeat_n(' ', spaces));
                source_column += spaces;
            } else {
                expanded.push_str(grapheme);
                source_column += display_width(grapheme);
            }
        }
        let mut rest = expanded.as_str();
        if rest.is_empty() {
            continue;
        }

        loop {
            if remaining == 0 {
                lines.push(Vec::new());
                remaining = max_width;
            }

            let mut piece = take_width_prefix(rest, remaining);
            if piece.is_empty() {
                if remaining < max_width {
                    lines.push(Vec::new());
                    remaining = max_width;
                    continue;
                }
                // A double-cell glyph in a one-cell viewport must make progress;
                // the terminal clips the glyph rather than splitting its bytes.
                piece = rest.graphemes(true).next().unwrap_or(rest);
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
    let syntax = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .and_then(|extension| assets.syntax_set.find_syntax_by_extension(extension))?;
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

pub(super) struct RecordedDiffHighlights {
    pub(super) before: Vec<ratatui::text::Line<'static>>,
    pub(super) after: Vec<ratatui::text::Line<'static>>,
}

pub(super) fn recorded_diff_highlights(
    file: &super::ui_diff_model::StructuredDiffFile,
    source: Option<&str>,
    theme: &crate::theme::Theme,
) -> RecordedDiffHighlights {
    use super::ui_diff_model::StructuredDiffDisplayRow;
    let mut before: Vec<String> = source.map_or_else(Vec::new, |source| {
        source.lines().map(str::to_string).collect()
    });
    if source.is_none() {
        for row in &file.rows {
            let item = match row {
                StructuredDiffDisplayRow::Context {
                    before_line: Some(line),
                    text,
                    ..
                } => Some((*line, text)),
                StructuredDiffDisplayRow::Changed {
                    before: Some(cell), ..
                } => cell.line_number.map(|line| (line, &cell.text)),
                _ => None,
            };
            if let Some((line, text)) = item.filter(|(line, _)| (1..=100_000).contains(line)) {
                before.resize(before.len().max(line), String::new());
                before[line - 1] = text.clone();
            }
        }
    }
    let mut after = Vec::new();
    let mut cursor = 0;
    for row in &file.rows {
        let (old_line, replacement) = match row {
            StructuredDiffDisplayRow::Context {
                before_line, text, ..
            } => (*before_line, Some(text)),
            StructuredDiffDisplayRow::Changed { before, after } => (
                before.as_ref().and_then(|cell| cell.line_number),
                after.as_ref().map(|cell| &cell.text),
            ),
            _ => continue,
        };
        if let Some(line) = old_line.filter(|line| *line > 0) {
            let start = (line - 1).min(before.len());
            if start > cursor {
                after.extend_from_slice(&before[cursor..start]);
            }
            cursor = line.min(before.len());
        }
        if let Some(text) = replacement {
            after.push(text.clone());
        }
    }
    after.extend_from_slice(&before[cursor..]);
    let language = file
        .after_path
        .as_deref()
        .or(file.before_path.as_deref())
        .or(Some(file.display_path.as_str()));
    let highlight = |lines: &[String]| {
        let body = lines.join("\n");
        super::super::ui_syntax_highlight::render_highlighted_code_block(
            language,
            &body,
            &body,
            "",
            theme.text.primary,
            theme,
        )
    };
    RecordedDiffHighlights {
        before: highlight(&before),
        after: highlight(&after),
    }
}

impl RecordedDiffHighlights {
    pub(super) fn before_line(
        &self,
        number: Option<usize>,
    ) -> Option<&ratatui::text::Line<'static>> {
        self.before.get(number?.checked_sub(1)?)
    }
    pub(super) fn after_line(
        &self,
        number: Option<usize>,
    ) -> Option<&ratatui::text::Line<'static>> {
        self.after.get(number?.checked_sub(1)?)
    }
}
