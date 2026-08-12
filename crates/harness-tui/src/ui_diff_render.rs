// allow: SIZE_OK — TUI diff rendering (indivisible view model)
use std::cmp::max;

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

use crate::theme::Theme;

use super::super::ui_chrome::{
    display_width, muted_meta_style, take_width_prefix, transcript_prefix_style,
    truncate_plain_text,
};
use super::ui_diff_model::{
    DiffCell, DiffSegmentKind, StructuredDiffDisplayRow, StructuredDiffFile, StructuredDiffModel,
};
use super::ui_diff_syntax::{
    highlight_diff_line_chunks, styled_chunks_to_spans, wrap_styled_chunks, StyledTextChunk,
};

#[expect(
    clippy::too_many_arguments,
    reason = "structured diff rendering keeps transcript/file/header toggles explicit at the call site"
)]
pub(super) fn render_structured_diff_model(
    model: &StructuredDiffModel,
    prefix: &str,
    width: u16,
    _force_stacked: bool,
    plain_numbered: bool,
    highlight_syntax: bool,
    show_file_header: bool,
    show_hunk_header: bool,
    theme: &Theme,
) -> (Vec<Line<'static>>, Vec<usize>) {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let mut lines = Vec::new();
    let mut hunk_offsets = Vec::new();

    for (file_index, file) in model.files.iter().enumerate() {
        if file_index > 0 {
            lines.push(Line::from(""));
        }

        let mut line_number_width = 1;

        for (row_index, row) in file.rows.iter().enumerate() {
            let syntax_path = file
                .after_path
                .as_deref()
                .or(file.before_path.as_deref())
                .or(Some(file.display_path.as_str()));
            match row {
                StructuredDiffDisplayRow::FileHeader => {
                    if show_file_header {
                        lines.push(render_diff_file_header(prefix, file, content_width, theme));
                    }
                }
                StructuredDiffDisplayRow::HunkHeader { text } => {
                    line_number_width = structured_diff_hunk_line_number_width(
                        file.rows.get(row_index + 1..).unwrap_or_default(),
                    );
                    hunk_offsets.push(lines.len());
                    if show_hunk_header {
                        lines.push(render_diff_hunk_header(
                            prefix,
                            text,
                            content_width,
                            line_number_width,
                            theme,
                        ));
                    }
                }
                StructuredDiffDisplayRow::Context {
                    before_line,
                    after_line,
                    text,
                } => {
                    lines.extend(render_unified_context_lines(
                        prefix,
                        (*after_line).or(*before_line),
                        text,
                        content_width,
                        line_number_width,
                        syntax_path,
                        highlight_syntax,
                        theme,
                    ));
                }
                StructuredDiffDisplayRow::Changed { before, after } => {
                    if let Some(before) = before {
                        lines.extend(render_unified_diff_cell_lines(
                            prefix,
                            before,
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            plain_numbered,
                            theme,
                        ));
                    }
                    if let Some(after) = after {
                        lines.extend(render_unified_diff_cell_lines(
                            prefix,
                            after,
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            plain_numbered,
                            theme,
                        ));
                    }
                }
                StructuredDiffDisplayRow::UnchangedGap { lines: unchanged } => {
                    lines.push(render_diff_unchanged_gap(
                        prefix,
                        *unchanged,
                        content_width,
                        line_number_width,
                        theme,
                    ));
                }
            }
        }
    }

    (lines, hunk_offsets)
}

fn wrap_plain_text_lines(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![String::new()];
    }
    if text.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let piece = take_width_prefix(rest, max_width);
        if piece.is_empty() {
            lines.push(String::new());
            break;
        }
        lines.push(piece.to_string());
        rest = &rest[piece.len()..];
    }

    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[expect(
    clippy::too_many_arguments,
    reason = "unified context rows keep number gutters and syntax inputs explicit at the call site"
)]
fn render_unified_context_lines(
    prefix: &str,
    line_number: Option<usize>,
    text: &str,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let palette = diff_row_palette(' ', theme);
    let text_width = width.saturating_sub(line_number_width + 2).max(1);
    let chunks = highlight_syntax
        .then(|| {
            highlight_diff_line_chunks(
                syntax_path,
                text,
                Some(palette.content_bg),
                theme.color_level(),
            )
        })
        .flatten()
        .unwrap_or_else(|| {
            vec![StyledTextChunk {
                text: text.to_string(),
                style: diff_segment_style(
                    DiffSegmentKind::Unchanged,
                    DiffSegmentKind::Unchanged,
                    Some(palette.content_bg),
                    theme,
                ),
            }]
        });
    wrap_styled_chunks(&chunks, text_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunks)| {
            let mut spans = unified_diff_gutter_spans(
                prefix,
                UnifiedDiffGutter {
                    marker: ' ',
                    line_number: (index == 0).then_some(line_number).flatten(),
                    show_marker: false,
                    line_number_width,
                    gutter_bg: Some(palette.gutter_bg),
                    content_bg: Some(palette.content_bg),
                },
                theme,
            );
            spans.extend(styled_chunks_to_spans(chunks));
            pad_diff_row_to_width_with_background(
                spans,
                display_width(prefix),
                width,
                Some(palette.content_bg),
            )
        })
        .collect()
}

fn render_unified_diff_cell_lines(
    prefix: &str,
    cell: &DiffCell,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    plain_numbered: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    if plain_numbered {
        return render_plain_numbered_diff_cell_lines(
            prefix,
            cell,
            width,
            line_number_width,
            theme,
        );
    }
    let accent_kind = if cell.marker == '-' {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };
    let palette = diff_row_palette(cell.marker, theme);
    let text_width = width.saturating_sub(line_number_width + 2).max(1);
    let show_marker = !diff_semantic_bands_visible(theme);
    let chunks = highlight_syntax
        .then(|| {
            highlight_diff_line_chunks(
                syntax_path,
                &cell.text,
                Some(palette.content_bg),
                theme.color_level(),
            )
        })
        .flatten()
        .unwrap_or_else(|| {
            cell.segments
                .iter()
                .map(|segment| StyledTextChunk {
                    text: segment.text.clone(),
                    style: diff_segment_style(
                        segment.kind,
                        accent_kind,
                        Some(palette.content_bg),
                        theme,
                    ),
                })
                .collect::<Vec<_>>()
        });
    wrap_styled_chunks(&chunks, text_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunks)| {
            let mut spans = unified_diff_gutter_spans(
                prefix,
                UnifiedDiffGutter {
                    marker: cell.marker,
                    line_number: (index == 0).then_some(cell.line_number).flatten(),
                    show_marker: index == 0 && show_marker,
                    line_number_width,
                    gutter_bg: Some(palette.gutter_bg),
                    content_bg: Some(palette.content_bg),
                },
                theme,
            );
            spans.extend(styled_chunks_to_spans(chunks));
            pad_diff_row_to_width_with_background(
                spans,
                display_width(prefix),
                width,
                Some(palette.content_bg),
            )
        })
        .collect()
}

fn render_plain_numbered_diff_cell_lines(
    prefix: &str,
    cell: &DiffCell,
    width: usize,
    line_number_width: usize,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let palette = diff_row_palette(cell.marker, theme);
    let text_width = width.saturating_sub(line_number_width + 2).max(1);
    let show_marker = !diff_semantic_bands_visible(theme);
    wrap_plain_text_lines(&cell.text, text_width)
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| {
            let mut spans = unified_diff_gutter_spans(
                prefix,
                UnifiedDiffGutter {
                    marker: cell.marker,
                    line_number: (index == 0).then_some(cell.line_number).flatten(),
                    show_marker: index == 0 && show_marker,
                    line_number_width,
                    gutter_bg: Some(palette.gutter_bg),
                    content_bg: Some(palette.content_bg),
                },
                theme,
            );
            spans.push(Span::styled(
                chunk,
                Style::default()
                    .fg(theme.text.primary)
                    .bg(palette.content_bg),
            ));
            pad_diff_row_to_width_with_background(
                spans,
                display_width(prefix),
                width,
                Some(palette.content_bg),
            )
        })
        .collect()
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DiffRowPalette {
    pub(crate) gutter_bg: Color,
    pub(crate) content_bg: Color,
}

struct UnifiedDiffGutter {
    marker: char,
    line_number: Option<usize>,
    show_marker: bool,
    line_number_width: usize,
    gutter_bg: Option<Color>,
    content_bg: Option<Color>,
}

fn unified_diff_gutter_spans(
    prefix: &str,
    gutter: UnifiedDiffGutter,
    theme: &Theme,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
        Span::styled(
            format_line_number(gutter.line_number, gutter.line_number_width),
            diff_line_number_style(gutter.marker, gutter.gutter_bg, theme),
        ),
        if gutter.show_marker {
            Span::styled(
                format!(" {}", gutter.marker),
                diff_marker_style(gutter.marker, gutter.content_bg, theme),
            )
        } else {
            Span::styled(
                "  ".to_string(),
                apply_optional_bg(Style::default(), gutter.content_bg),
            )
        },
    ]
}

pub(super) fn diff_marker_style(marker: char, row_bg: Option<Color>, theme: &Theme) -> Style {
    let style = match marker {
        '+' => Style::default().fg(reference_diff_highlight_added(theme)),
        '-' => Style::default().fg(reference_diff_highlight_removed(theme)),
        _ => muted_meta_style(theme),
    };
    apply_optional_bg(style, row_bg)
}

fn diff_line_number_style(_marker: char, row_bg: Option<Color>, theme: &Theme) -> Style {
    apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg)
}

pub(super) fn diff_segment_style(
    kind: DiffSegmentKind,
    accent_kind: DiffSegmentKind,
    row_bg: Option<Color>,
    theme: &Theme,
) -> Style {
    match kind {
        DiffSegmentKind::Unchanged => {
            apply_optional_bg(Style::default().fg(theme.text.primary), row_bg)
        }
        DiffSegmentKind::Removed => {
            let fg = match accent_kind {
                DiffSegmentKind::Removed => theme.text.primary,
                _ => reference_diff_highlight_removed(theme),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
        DiffSegmentKind::Added => {
            let fg = match accent_kind {
                DiffSegmentKind::Added => theme.text.primary,
                _ => reference_diff_highlight_added(theme),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
    }
}

fn render_diff_unchanged_gap(
    prefix: &str,
    unchanged: usize,
    width: usize,
    line_number_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let palette = diff_hunk_palette(theme);
    let content = format!(
        "… {unchanged} unchanged line{}",
        if unchanged == 1 { "" } else { "s" }
    );
    let header_width = width.saturating_sub(line_number_width + 2);
    let mut spans = unified_diff_gutter_spans(
        prefix,
        UnifiedDiffGutter {
            marker: ' ',
            line_number: None,
            show_marker: false,
            line_number_width,
            gutter_bg: Some(palette.gutter_bg),
            content_bg: Some(palette.content_bg),
        },
        theme,
    );
    spans.push(Span::styled(
        truncate_plain_text(&content, header_width),
        muted_meta_style(theme).bg(palette.content_bg),
    ));
    pad_diff_row_to_width_with_background(
        spans,
        display_width(prefix),
        width,
        Some(palette.content_bg),
    )
}

pub(super) fn render_diff_hunk_header(
    prefix: &str,
    text: &str,
    width: usize,
    line_number_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let palette = diff_hunk_palette(theme);
    let header_width = width.saturating_sub(line_number_width + 2);
    let mut spans = unified_diff_gutter_spans(
        prefix,
        UnifiedDiffGutter {
            marker: ' ',
            line_number: None,
            show_marker: false,
            line_number_width,
            gutter_bg: Some(palette.gutter_bg),
            content_bg: Some(palette.content_bg),
        },
        theme,
    );
    spans.push(Span::styled(
        truncate_plain_text(&format!("⋮ {text}"), header_width),
        apply_optional_bg(
            Style::default().fg(reference_diff_hunk_header(theme)),
            Some(palette.content_bg),
        ),
    ));
    pad_diff_row_to_width_with_background(
        spans,
        display_width(prefix),
        width,
        Some(palette.content_bg),
    )
}

fn render_diff_file_header(
    prefix: &str,
    file: &StructuredDiffFile,
    content_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let row_bg = Some(diff_panel_background(theme));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(
        "← Patched ".to_string(),
        apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg),
    ));

    if file.before_path != file.after_path {
        if let Some(before_path) = file.before_path.as_ref() {
            spans.extend(render_diff_path_spans(
                before_path,
                muted_meta_style(theme),
                Style::default().fg(theme.text.primary),
                row_bg,
            ));
            spans.push(Span::styled(
                " → ".to_string(),
                apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg),
            ));
        }
        spans.extend(render_diff_path_spans(
            file.after_path.as_deref().unwrap_or(&file.display_path),
            muted_meta_style(theme),
            Style::default().fg(theme.text.primary),
            row_bg,
        ));
    } else {
        spans.extend(render_diff_path_spans(
            &file.display_path,
            muted_meta_style(theme),
            Style::default().fg(theme.text.primary),
            row_bg,
        ));
    }
    pad_diff_row_to_width_with_background(spans, display_width(prefix), content_width, row_bg)
}

fn render_diff_path_spans(
    path: &str,
    directory_style: Style,
    filename_style: Style,
    row_bg: Option<Color>,
) -> Vec<Span<'static>> {
    let (directory, filename) = split_diff_path(path);
    let mut spans = Vec::new();
    if !directory.is_empty() {
        spans.push(Span::styled(
            format!("{directory}/"),
            apply_optional_bg(directory_style, row_bg),
        ));
    }
    spans.push(Span::styled(
        filename.to_string(),
        apply_optional_bg(filename_style, row_bg),
    ));
    spans
}

fn pad_diff_row_to_width_with_background(
    mut spans: Vec<Span<'static>>,
    prefix_width: usize,
    content_width: usize,
    background: Option<Color>,
) -> Line<'static> {
    let used_width = spans.iter().map(Span::width).sum::<usize>();
    let target_width = prefix_width.saturating_add(content_width);
    if used_width < target_width {
        spans.push(padded_diff_span(target_width - used_width, background));
    }
    Line::from(spans)
}

fn structured_diff_hunk_max_line_number(rows: &[StructuredDiffDisplayRow]) -> usize {
    rows.iter()
        .take_while(|row| !matches!(row, StructuredDiffDisplayRow::HunkHeader { .. }))
        .fold(0usize, |current, row| match row {
            StructuredDiffDisplayRow::Context {
                before_line,
                after_line,
                ..
            } => current
                .max(before_line.unwrap_or(0))
                .max(after_line.unwrap_or(0)),
            StructuredDiffDisplayRow::Changed { before, after } => current
                .max(
                    before
                        .as_ref()
                        .and_then(|cell| cell.line_number)
                        .unwrap_or(0),
                )
                .max(
                    after
                        .as_ref()
                        .and_then(|cell| cell.line_number)
                        .unwrap_or(0),
                ),
            StructuredDiffDisplayRow::FileHeader
            | StructuredDiffDisplayRow::HunkHeader { .. }
            | StructuredDiffDisplayRow::UnchangedGap { .. } => current,
        })
}

fn structured_diff_hunk_line_number_width(rows: &[StructuredDiffDisplayRow]) -> usize {
    max(
        1,
        structured_diff_hunk_max_line_number(rows).to_string().len(),
    )
}

fn split_diff_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn format_line_number(line: Option<usize>, width: usize) -> String {
    match line {
        Some(value) => format!("{value:>width$}"),
        None => " ".repeat(width),
    }
}

fn padded_diff_span(width: usize, row_bg: Option<Color>) -> Span<'static> {
    if let Some(background) = row_bg {
        Span::styled(" ".repeat(width), Style::default().bg(background))
    } else {
        Span::raw(" ".repeat(width))
    }
}
pub(super) fn diff_row_palette(marker: char, theme: &Theme) -> DiffRowPalette {
    match marker {
        '+' => DiffRowPalette {
            gutter_bg: reference_diff_added_line_number_bg(theme),
            content_bg: reference_diff_added_bg(theme),
        },
        '-' => DiffRowPalette {
            gutter_bg: reference_diff_removed_line_number_bg(theme),
            content_bg: reference_diff_removed_bg(theme),
        },
        _ => DiffRowPalette {
            gutter_bg: diff_context_background(theme),
            content_bg: diff_panel_background(theme),
        },
    }
}

fn diff_semantic_bands_visible(theme: &Theme) -> bool {
    let added = diff_row_palette('+', theme);
    let removed = diff_row_palette('-', theme);
    added.content_bg != removed.content_bg || added.gutter_bg != removed.gutter_bg
}

fn diff_panel_background(theme: &Theme) -> Color {
    theme.surface.panel
}

fn diff_context_background(theme: &Theme) -> Color {
    theme.surface.panel
}

pub(super) fn diff_hunk_palette(theme: &Theme) -> DiffRowPalette {
    DiffRowPalette {
        gutter_bg: diff_context_background(theme),
        content_bg: diff_context_background(theme),
    }
}

pub(super) const fn reference_diff_added_bg(theme: &Theme) -> Color {
    theme.reference_terminal.diff_added
}

pub(super) const fn reference_diff_removed_bg(theme: &Theme) -> Color {
    theme.reference_terminal.diff_removed
}

pub(super) const fn reference_diff_added_line_number_bg(theme: &Theme) -> Color {
    theme.reference_terminal.diff_added_gutter
}

pub(super) const fn reference_diff_removed_line_number_bg(theme: &Theme) -> Color {
    theme.reference_terminal.diff_removed_gutter
}

pub(super) const fn reference_diff_highlight_added(theme: &Theme) -> Color {
    theme.reference_terminal.diff_added_highlight
}

pub(super) const fn reference_diff_highlight_removed(theme: &Theme) -> Color {
    theme.reference_terminal.diff_removed_highlight
}

pub(super) const fn reference_diff_hunk_header(theme: &Theme) -> Color {
    theme.reference_terminal.diff_hunk_header
}

fn apply_optional_bg(style: Style, background: Option<Color>) -> Style {
    background.map_or(style, |bg| style.bg(bg))
}
