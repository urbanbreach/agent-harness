use imara_diff::{Algorithm, Diff, InternedInput};
use std::cmp::max;
use std::path::Path;
use std::str::FromStr;
use std::sync::OnceLock;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SyntectColor, FontStyle as SyntectFontStyle, ScopeSelectors,
    StyleModifier as SyntectStyleModifier, Theme as SyntectTheme, ThemeItem,
    ThemeSettings as SyntectThemeSettings,
};
use syntect::parsing::SyntaxSet;

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::ui_chrome::{
    display_width, muted_meta_style, take_width_prefix, transcript_prefix_style,
    truncate_plain_text,
};
use crate::theme::{Theme, DIFF_SIDE_BY_SIDE_MIN_WIDTH};

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPatchFile {
    display_path: String,
    before_label: String,
    after_label: String,
    hunks: Vec<ParsedPatchHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedPatchHunk {
    header: String,
    before_lines: Vec<String>,
    after_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredDiffModel {
    files: Vec<StructuredDiffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StructuredDiffFile {
    display_path: String,
    before_path: Option<String>,
    after_path: Option<String>,
    additions: usize,
    removals: usize,
    rows: Vec<StructuredDiffDisplayRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum StructuredDiffDisplayRow {
    FileHeader,
    HunkHeader {
        text: String,
    },
    Context {
        before_line: Option<usize>,
        after_line: Option<usize>,
        text: String,
    },
    Changed {
        before: Option<DiffCell>,
        after: Option<DiffCell>,
    },
    Spacer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffCell {
    marker: char,
    line_number: Option<usize>,
    text: String,
    segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DiffSegment {
    kind: DiffSegmentKind,
    text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiffSegmentKind {
    Unchanged,
    Removed,
    Added,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct StructuredDiffRenderOptions {
    pub force_stacked: bool,
    pub highlight_intraline: bool,
    pub highlight_syntax: bool,
    pub show_file_header: bool,
    pub show_hunk_header: bool,
}

pub(super) fn render_structured_diff_lines(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    render_structured_diff_lines_with_options(
        diff_content,
        fallback_path,
        prefix,
        width,
        StructuredDiffRenderOptions {
            force_stacked,
            highlight_intraline: true,
            highlight_syntax: false,
            show_file_header: true,
            show_hunk_header: true,
        },
        theme,
    )
}

pub(super) fn render_structured_diff_lines_with_options(
    diff_content: &str,
    fallback_path: Option<&str>,
    prefix: &str,
    width: u16,
    options: StructuredDiffRenderOptions,
    theme: &Theme,
) -> Option<Vec<Line<'static>>> {
    let model =
        structured_diff_model_from_patch(diff_content, fallback_path, options.highlight_intraline)?;
    Some(render_structured_diff_model(
        &model,
        prefix,
        width,
        options.force_stacked,
        options.highlight_syntax,
        options.show_file_header,
        options.show_hunk_header,
        theme,
    ))
}

pub(super) fn structured_diff_stats(
    diff_content: &str,
    fallback_path: Option<&str>,
    highlight_intraline: bool,
) -> Option<(usize, usize)> {
    structured_diff_model_from_patch(diff_content, fallback_path, highlight_intraline).map(
        |model| {
            model
                .files
                .into_iter()
                .fold((0usize, 0usize), |(additions, removals), file| {
                    (
                        additions.saturating_add(file.additions),
                        removals.saturating_add(file.removals),
                    )
                })
        },
    )
}

fn structured_diff_model_from_patch(
    diff_content: &str,
    fallback_path: Option<&str>,
    highlight_intraline: bool,
) -> Option<StructuredDiffModel> {
    let files = parse_unified_diff_files(diff_content, fallback_path)?;
    Some(StructuredDiffModel {
        files: files
            .into_iter()
            .map(|file| build_structured_diff_file(file, highlight_intraline))
            .collect(),
    })
}

fn build_structured_diff_file(
    file: ParsedPatchFile,
    highlight_intraline: bool,
) -> StructuredDiffFile {
    let mut rows = Vec::new();
    let mut additions = 0;
    let mut removals = 0;

    for (index, hunk) in file.hunks.into_iter().enumerate() {
        if index == 0 {
            rows.push(StructuredDiffDisplayRow::FileHeader);
        } else {
            rows.push(StructuredDiffDisplayRow::Spacer);
        }
        rows.push(StructuredDiffDisplayRow::HunkHeader {
            text: hunk.header.clone(),
        });

        let aligned = align_patch_hunk(&hunk, highlight_intraline);
        additions += aligned.additions;
        removals += aligned.removals;
        rows.extend(aligned.rows);
    }

    StructuredDiffFile {
        display_path: file.display_path,
        before_path: normalize_patch_label(&file.before_label),
        after_path: normalize_patch_label(&file.after_label),
        additions,
        removals,
        rows,
    }
}

struct AlignedHunk {
    rows: Vec<StructuredDiffDisplayRow>,
    additions: usize,
    removals: usize,
}

fn align_patch_hunk(hunk: &ParsedPatchHunk, highlight_intraline: bool) -> AlignedHunk {
    let (mut before_line, mut after_line) = parse_hunk_start_lines(&hunk.header);
    let before_text = hunk.before_lines.join("\n");
    let after_text = hunk.after_lines.join("\n");
    let input = InternedInput::new(
        imara_diff::sources::lines(&before_text),
        imara_diff::sources::lines(&after_text),
    );
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut rows = Vec::new();
    let mut before_idx = 0usize;
    let mut after_idx = 0usize;

    for diff_hunk in diff.hunks() {
        let next_before = usize::try_from(diff_hunk.before.start).unwrap_or(before_idx);
        let next_after = usize::try_from(diff_hunk.after.start).unwrap_or(after_idx);
        let unchanged = max(
            next_before.saturating_sub(before_idx),
            next_after.saturating_sub(after_idx),
        );

        for offset in 0..unchanged {
            let line = hunk
                .before_lines
                .get(before_idx + offset)
                .or_else(|| hunk.after_lines.get(after_idx + offset))
                .cloned()
                .unwrap_or_default();
            rows.push(StructuredDiffDisplayRow::Context {
                before_line: visible_diff_line_number(before_line),
                after_line: visible_diff_line_number(after_line),
                text: line,
            });
            before_line = before_line.saturating_add(1);
            after_line = after_line.saturating_add(1);
        }
        before_idx = next_before;
        after_idx = next_after;

        let removed_end = usize::try_from(diff_hunk.before.end).unwrap_or(before_idx);
        let added_end = usize::try_from(diff_hunk.after.end).unwrap_or(after_idx);
        let removed = &hunk.before_lines[before_idx..removed_end];
        let added = &hunk.after_lines[after_idx..added_end];

        for pair_index in 0..max(removed.len(), added.len()) {
            match (removed.get(pair_index), added.get(pair_index)) {
                (Some(before), Some(after)) if before == after => {
                    rows.push(StructuredDiffDisplayRow::Context {
                        before_line: visible_diff_line_number(before_line),
                        after_line: visible_diff_line_number(after_line),
                        text: before.clone(),
                    });
                    before_line = before_line.saturating_add(1);
                    after_line = after_line.saturating_add(1);
                }
                (Some(before), Some(after)) => {
                    let (before_segments, after_segments) = if highlight_intraline {
                        word_diff_segments(before, after)
                    } else {
                        (
                            vec![DiffSegment {
                                kind: DiffSegmentKind::Unchanged,
                                text: before.clone(),
                            }],
                            vec![DiffSegment {
                                kind: DiffSegmentKind::Unchanged,
                                text: after.clone(),
                            }],
                        )
                    };
                    rows.push(StructuredDiffDisplayRow::Changed {
                        before: Some(DiffCell {
                            marker: '-',
                            line_number: visible_diff_line_number(before_line),
                            text: before.clone(),
                            segments: before_segments,
                        }),
                        after: Some(DiffCell {
                            marker: '+',
                            line_number: visible_diff_line_number(after_line),
                            text: after.clone(),
                            segments: after_segments,
                        }),
                    });
                    before_line = before_line.saturating_add(1);
                    after_line = after_line.saturating_add(1);
                }
                (Some(before), None) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: Some(DiffCell {
                        marker: '-',
                        line_number: visible_diff_line_number(before_line),
                        text: before.clone(),
                        segments: vec![DiffSegment {
                            kind: if highlight_intraline {
                                DiffSegmentKind::Removed
                            } else {
                                DiffSegmentKind::Unchanged
                            },
                            text: before.clone(),
                        }],
                    }),
                    after: None,
                }),
                (None, Some(after)) => rows.push(StructuredDiffDisplayRow::Changed {
                    before: None,
                    after: Some(DiffCell {
                        marker: '+',
                        line_number: visible_diff_line_number(after_line),
                        text: after.clone(),
                        segments: vec![DiffSegment {
                            kind: if highlight_intraline {
                                DiffSegmentKind::Added
                            } else {
                                DiffSegmentKind::Unchanged
                            },
                            text: after.clone(),
                        }],
                    }),
                }),
                (None, None) => {}
            }
            match (removed.get(pair_index), added.get(pair_index)) {
                (Some(_), Some(_)) => {}
                (Some(_), None) => before_line = before_line.saturating_add(1),
                (None, Some(_)) => after_line = after_line.saturating_add(1),
                (None, None) => {}
            }
        }

        before_idx = removed_end;
        after_idx = added_end;
    }

    let trailing = max(
        hunk.before_lines.len().saturating_sub(before_idx),
        hunk.after_lines.len().saturating_sub(after_idx),
    );
    for offset in 0..trailing {
        let line = hunk
            .before_lines
            .get(before_idx + offset)
            .or_else(|| hunk.after_lines.get(after_idx + offset))
            .cloned()
            .unwrap_or_default();
        rows.push(StructuredDiffDisplayRow::Context {
            before_line: visible_diff_line_number(before_line),
            after_line: visible_diff_line_number(after_line),
            text: line,
        });
        before_line = before_line.saturating_add(1);
        after_line = after_line.saturating_add(1);
    }

    AlignedHunk {
        rows,
        additions: usize::try_from(diff.count_additions()).unwrap_or(usize::MAX),
        removals: usize::try_from(diff.count_removals()).unwrap_or(usize::MAX),
    }
}

fn word_diff_segments(before: &str, after: &str) -> (Vec<DiffSegment>, Vec<DiffSegment>) {
    let before_tokens = tokenize_diff_words(before);
    let after_tokens = tokenize_diff_words(after);

    if before_tokens.is_empty() || after_tokens.is_empty() {
        return (
            vec![DiffSegment {
                kind: DiffSegmentKind::Removed,
                text: before.to_string(),
            }],
            vec![DiffSegment {
                kind: DiffSegmentKind::Added,
                text: after.to_string(),
            }],
        );
    }

    let mut input = InternedInput::default();
    input.update_before(before_tokens.clone().into_iter());
    input.update_after(after_tokens.clone().into_iter());
    let mut diff = Diff::compute(Algorithm::Histogram, &input);
    diff.postprocess_lines(&input);

    let mut before_segments = Vec::new();
    let mut after_segments = Vec::new();
    let mut before_idx = 0usize;
    let mut after_idx = 0usize;

    for hunk in diff.hunks() {
        let next_before = usize::try_from(hunk.before.start).unwrap_or(before_idx);
        let next_after = usize::try_from(hunk.after.start).unwrap_or(after_idx);
        push_diff_segments(
            &mut before_segments,
            &before_tokens[before_idx..next_before],
            DiffSegmentKind::Unchanged,
        );
        push_diff_segments(
            &mut after_segments,
            &after_tokens[after_idx..next_after],
            DiffSegmentKind::Unchanged,
        );
        push_diff_segments(
            &mut before_segments,
            &before_tokens[next_before..usize::try_from(hunk.before.end).unwrap_or(next_before)],
            DiffSegmentKind::Removed,
        );
        push_diff_segments(
            &mut after_segments,
            &after_tokens[next_after..usize::try_from(hunk.after.end).unwrap_or(next_after)],
            DiffSegmentKind::Added,
        );
        before_idx = usize::try_from(hunk.before.end).unwrap_or(before_idx);
        after_idx = usize::try_from(hunk.after.end).unwrap_or(after_idx);
    }

    push_diff_segments(
        &mut before_segments,
        &before_tokens[before_idx..],
        DiffSegmentKind::Unchanged,
    );
    push_diff_segments(
        &mut after_segments,
        &after_tokens[after_idx..],
        DiffSegmentKind::Unchanged,
    );

    (before_segments, after_segments)
}

fn push_diff_segments(target: &mut Vec<DiffSegment>, tokens: &[String], kind: DiffSegmentKind) {
    if tokens.is_empty() {
        return;
    }
    let chunk = tokens.concat();
    if let Some(previous) = target.last_mut() {
        if previous.kind == kind {
            previous.text.push_str(&chunk);
            return;
        }
    }
    target.push(DiffSegment { kind, text: chunk });
}

fn tokenize_diff_words(input: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut kind: Option<u8> = None;

    for ch in input.chars() {
        let next_kind = if ch.is_whitespace() {
            0
        } else if ch.is_alphanumeric() || ch == '_' {
            1
        } else {
            2
        };

        if next_kind == 2 {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
            tokens.push(ch.to_string());
            kind = None;
            continue;
        }

        if kind.is_some_and(|existing| existing != next_kind) && !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
        current.push(ch);
        kind = Some(next_kind);
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}

#[expect(
    clippy::too_many_arguments,
    reason = "structured diff rendering keeps transcript/file/header toggles explicit at the call site"
)]
fn render_structured_diff_model(
    model: &StructuredDiffModel,
    prefix: &str,
    width: u16,
    force_stacked: bool,
    highlight_syntax: bool,
    show_file_header: bool,
    show_hunk_header: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let prefix_width = display_width(prefix);
    let content_width = usize::from(width).saturating_sub(prefix_width).max(1);
    let wide = !force_stacked && content_width >= usize::from(DIFF_SIDE_BY_SIDE_MIN_WIDTH);
    let mut lines = Vec::new();

    for (file_index, file) in model.files.iter().enumerate() {
        if file_index > 0 {
            lines.push(Line::from(""));
        }

        let line_number_width = structured_diff_line_number_width(file);

        for row in &file.rows {
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
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            Some(&DiffCell {
                                marker: ' ',
                                line_number: *before_line,
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            Some(&DiffCell {
                                marker: ' ',
                                line_number: *after_line,
                                text: text.clone(),
                                segments: vec![DiffSegment {
                                    kind: DiffSegmentKind::Unchanged,
                                    text: text.clone(),
                                }],
                            }),
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    } else {
                        lines.extend(render_stacked_diff_lines(
                            prefix,
                            ' ',
                            *before_line,
                            *after_line,
                            text,
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    }
                }
                StructuredDiffDisplayRow::Changed { before, after } => {
                    if wide {
                        lines.push(render_wide_diff_row(
                            prefix,
                            before.as_ref(),
                            after.as_ref(),
                            content_width,
                            line_number_width,
                            syntax_path,
                            highlight_syntax,
                            theme,
                        ));
                    } else {
                        if let Some(before) = before {
                            lines.extend(render_stacked_diff_cell_lines(
                                prefix,
                                before,
                                content_width,
                                line_number_width,
                                syntax_path,
                                highlight_syntax,
                                theme,
                            ));
                        }
                        if let Some(after) = after {
                            lines.extend(render_stacked_diff_cell_lines(
                                prefix,
                                after,
                                content_width,
                                line_number_width,
                                syntax_path,
                                highlight_syntax,
                                theme,
                            ));
                        }
                    }
                }
                StructuredDiffDisplayRow::Spacer => lines.push(Line::from("")),
            }
        }
    }

    lines
}

#[expect(
    clippy::too_many_arguments,
    reason = "wide diff rows need explicit geometry, syntax, and palette inputs to preserve the rendering contract"
)]
fn render_wide_diff_row(
    prefix: &str,
    before: Option<&DiffCell>,
    after: Option<&DiffCell>,
    content_width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Line<'static> {
    let separator = "  ";
    let column_width = content_width.saturating_sub(display_width(separator)) / 2;
    let before_palette = before
        .map(|cell| diff_row_palette(cell.marker, theme))
        .unwrap_or_else(|| diff_row_palette(' ', theme));
    let after_palette = after
        .map(|cell| diff_row_palette(cell.marker, theme))
        .unwrap_or_else(|| diff_row_palette(' ', theme));
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.extend(render_diff_cell(
        before,
        column_width,
        true,
        line_number_width,
        before_palette,
        syntax_path,
        highlight_syntax,
        theme,
    ));
    spans.push(Span::styled(
        separator.to_string(),
        apply_optional_bg(muted_meta_style(theme), Some(diff_panel_background(theme))),
    ));
    spans.extend(render_diff_cell(
        after,
        column_width,
        false,
        line_number_width,
        after_palette,
        syntax_path,
        highlight_syntax,
        theme,
    ));
    Line::from(spans)
}

#[expect(
    clippy::too_many_arguments,
    reason = "diff cells keep width, side, syntax, and palette state explicit to avoid hidden layout coupling"
)]
fn render_diff_cell(
    cell: Option<&DiffCell>,
    width: usize,
    is_before: bool,
    line_number_width: usize,
    palette: DiffRowPalette,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let Some(cell) = cell else {
        return vec![padded_diff_span(width, Some(palette.content_bg))];
    };

    let marker_width = 2usize;
    let number_width = line_number_width.saturating_add(1);
    let text_width = width.saturating_sub(number_width + marker_width);
    let accent_kind = if is_before {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };

    let mut spans = vec![Span::styled(
        format_line_number(cell.line_number, line_number_width),
        diff_line_number_style(cell.marker, Some(palette.gutter_bg), theme),
    )];
    spans.push(Span::styled(
        format!("{} ", cell.marker),
        diff_marker_style(cell.marker, Some(palette.content_bg), theme),
    ));
    if highlight_syntax {
        if let Some(chunks) =
            highlight_diff_line_chunks(syntax_path, &cell.text, Some(palette.content_bg))
        {
            spans.extend(truncate_styled_chunks(&chunks, text_width));
        } else {
            spans.extend(truncate_diff_segments(
                &cell.segments,
                text_width,
                accent_kind,
                Some(palette.content_bg),
                theme,
            ));
        }
    } else {
        spans.extend(truncate_diff_segments(
            &cell.segments,
            text_width,
            accent_kind,
            Some(palette.content_bg),
            theme,
        ));
    }
    let used_width = spans.iter().map(Span::width).sum::<usize>();
    if used_width < width {
        spans.push(padded_diff_span(
            width - used_width,
            Some(palette.content_bg),
        ));
    }
    spans
}

fn truncate_diff_segments(
    segments: &[DiffSegment],
    max_width: usize,
    accent_kind: DiffSegmentKind,
    row_bg: Option<Color>,
    theme: &Theme,
) -> Vec<Span<'static>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut rendered = Vec::new();
    let mut used = 0usize;

    for segment in segments {
        if used >= max_width {
            break;
        }
        let remaining = max_width - used;
        let text = truncate_plain_text(&segment.text, remaining);
        used += display_width(&text);
        rendered.push(Span::styled(
            text,
            diff_segment_style(segment.kind, accent_kind, row_bg, theme),
        ));
    }

    rendered
}

fn wrap_diff_segments(segments: &[DiffSegment], max_width: usize) -> Vec<Vec<DiffSegment>> {
    if max_width == 0 {
        return vec![Vec::new()];
    }

    let mut lines = vec![Vec::new()];
    let mut remaining = max_width;

    for segment in segments {
        let mut rest = segment.text.as_str();
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

            push_wrapped_segment(&mut lines[..], segment.kind, piece);
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

fn push_wrapped_segment(lines: &mut [Vec<DiffSegment>], kind: DiffSegmentKind, text: &str) {
    let Some(current) = lines.last_mut() else {
        return;
    };
    if let Some(last) = current.last_mut() {
        if last.kind == kind {
            last.text.push_str(text);
            return;
        }
    }
    current.push(DiffSegment {
        kind,
        text: text.to_string(),
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "stacked diff rows keep number gutters and width math explicit at the call site"
)]
fn render_stacked_diff_lines(
    prefix: &str,
    marker: char,
    before_line: Option<usize>,
    after_line: Option<usize>,
    text: &str,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let palette = diff_row_palette(marker, theme);
    let number_width = line_number_width.saturating_add(1) * 2;
    let text_width = width.saturating_sub(number_width + 2);
    let rows = if highlight_syntax {
        highlight_diff_line_chunks(syntax_path, text, Some(palette.content_bg))
            .map(|chunks| wrap_styled_chunks(&chunks, text_width))
    } else {
        None
    };
    rows.unwrap_or_else(|| {
        wrap_plain_text_lines(text, text_width)
            .into_iter()
            .map(|chunk| {
                vec![StyledTextChunk {
                    text: chunk,
                    style: diff_segment_style(
                        DiffSegmentKind::Unchanged,
                        DiffSegmentKind::Unchanged,
                        Some(palette.content_bg),
                        theme,
                    ),
                }]
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .enumerate()
    .map(|(index, chunks)| {
        let mut spans = stacked_diff_gutter_spans(
            prefix,
            StackedDiffGutter {
                marker,
                before_line: (index == 0).then_some(before_line).flatten(),
                after_line: (index == 0).then_some(after_line).flatten(),
                show_marker: index == 0,
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

fn render_stacked_diff_cell_lines(
    prefix: &str,
    cell: &DiffCell,
    width: usize,
    line_number_width: usize,
    syntax_path: Option<&str>,
    highlight_syntax: bool,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let accent_kind = if cell.marker == '-' {
        DiffSegmentKind::Removed
    } else {
        DiffSegmentKind::Added
    };
    let palette = diff_row_palette(cell.marker, theme);
    let number_width = line_number_width.saturating_add(1) * 2;
    let text_width = width.saturating_sub(number_width + 2);
    let rows = if highlight_syntax {
        highlight_diff_line_chunks(syntax_path, &cell.text, Some(palette.content_bg))
            .map(|chunks| wrap_styled_chunks(&chunks, text_width))
    } else {
        None
    };
    rows.unwrap_or_else(|| {
        wrap_diff_segments(&cell.segments, text_width)
            .into_iter()
            .map(|segments| {
                segments
                    .into_iter()
                    .map(|segment| StyledTextChunk {
                        text: segment.text,
                        style: diff_segment_style(
                            segment.kind,
                            accent_kind,
                            Some(palette.content_bg),
                            theme,
                        ),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
    .into_iter()
    .enumerate()
    .map(|(index, chunks)| {
        let mut spans = stacked_diff_gutter_spans(
            prefix,
            StackedDiffGutter {
                marker: cell.marker,
                before_line: (index == 0 && cell.marker == '-')
                    .then_some(cell.line_number)
                    .flatten(),
                after_line: (index == 0 && cell.marker == '+')
                    .then_some(cell.line_number)
                    .flatten(),
                show_marker: index == 0,
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

#[derive(Debug, Clone, Copy)]
struct DiffRowPalette {
    gutter_bg: Color,
    content_bg: Color,
}

#[derive(Debug, Clone)]
struct StyledTextChunk {
    text: String,
    style: Style,
}

struct StackedDiffGutter {
    marker: char,
    before_line: Option<usize>,
    after_line: Option<usize>,
    show_marker: bool,
    line_number_width: usize,
    gutter_bg: Option<Color>,
    content_bg: Option<Color>,
}

fn stacked_diff_gutter_spans(
    prefix: &str,
    gutter: StackedDiffGutter,
    theme: &Theme,
) -> Vec<Span<'static>> {
    vec![
        Span::styled(prefix.to_string(), transcript_prefix_style(theme)),
        Span::styled(
            format_line_number(gutter.before_line, gutter.line_number_width),
            diff_line_number_style(gutter.marker, gutter.gutter_bg, theme),
        ),
        Span::styled(
            format_line_number(gutter.after_line, gutter.line_number_width),
            diff_line_number_style(gutter.marker, gutter.gutter_bg, theme),
        ),
        if gutter.show_marker {
            Span::styled(
                format!("{} ", gutter.marker),
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

fn diff_marker_style(marker: char, row_bg: Option<Color>, _theme: &Theme) -> Style {
    let style = match marker {
        '+' => Style::default().fg(reference_diff_highlight_added()),
        '-' => Style::default().fg(reference_diff_highlight_removed()),
        _ => muted_meta_style(_theme),
    };
    apply_optional_bg(style, row_bg)
}

fn diff_line_number_style(_marker: char, row_bg: Option<Color>, theme: &Theme) -> Style {
    apply_optional_bg(Style::default().fg(theme.text.secondary), row_bg)
}

fn diff_segment_style(
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
                _ => reference_diff_highlight_removed(),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
        DiffSegmentKind::Added => {
            let fg = match accent_kind {
                DiffSegmentKind::Added => theme.text.primary,
                _ => reference_diff_highlight_added(),
            };
            apply_optional_bg(Style::default().fg(fg), row_bg)
        }
    }
}

fn render_diff_hunk_header(
    prefix: &str,
    text: &str,
    width: usize,
    line_number_width: usize,
    theme: &Theme,
) -> Line<'static> {
    let palette = diff_hunk_palette(theme);
    let header_width = width.saturating_sub(line_number_width.saturating_add(1) * 2 + 2);
    let mut spans = vec![Span::styled(
        prefix.to_string(),
        transcript_prefix_style(theme),
    )];
    spans.push(Span::styled(
        format_line_number(None, line_number_width),
        diff_line_number_style(' ', Some(palette.gutter_bg), theme),
    ));
    spans.push(Span::styled(
        format_line_number(None, line_number_width),
        diff_line_number_style(' ', Some(palette.gutter_bg), theme),
    ));
    spans.push(Span::styled(
        "  ".to_string(),
        apply_optional_bg(Style::default(), Some(palette.content_bg)),
    ));
    spans.push(Span::styled(
        truncate_plain_text(&format!("⋮ {text}"), header_width),
        apply_optional_bg(
            Style::default().fg(reference_diff_hunk_header()),
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

fn structured_diff_line_number_width(file: &StructuredDiffFile) -> usize {
    let max_line = file.rows.iter().fold(0usize, |current, row| match row {
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
        _ => current,
    });
    max(4, max_line.to_string().len())
}

fn split_diff_path(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

fn format_line_number(line: Option<usize>, width: usize) -> String {
    match line {
        Some(value) => format!("{value:>width$} "),
        None => " ".repeat(width.saturating_add(1)),
    }
}

fn padded_diff_span(width: usize, row_bg: Option<Color>) -> Span<'static> {
    if let Some(background) = row_bg {
        Span::styled(" ".repeat(width), Style::default().bg(background))
    } else {
        Span::raw(" ".repeat(width))
    }
}

fn styled_chunks_to_spans(chunks: Vec<StyledTextChunk>) -> Vec<Span<'static>> {
    chunks
        .into_iter()
        .map(|chunk| Span::styled(chunk.text, chunk.style))
        .collect()
}

fn truncate_styled_chunks(chunks: &[StyledTextChunk], max_width: usize) -> Vec<Span<'static>> {
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

fn wrap_styled_chunks(chunks: &[StyledTextChunk], max_width: usize) -> Vec<Vec<StyledTextChunk>> {
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

fn highlight_diff_line_chunks(
    path: Option<&str>,
    text: &str,
    row_bg: Option<Color>,
) -> Option<Vec<StyledTextChunk>> {
    let path = path?;
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

fn diff_row_palette(marker: char, theme: &Theme) -> DiffRowPalette {
    match marker {
        '+' => DiffRowPalette {
            gutter_bg: reference_diff_added_line_number_bg(),
            content_bg: reference_diff_added_bg(),
        },
        '-' => DiffRowPalette {
            gutter_bg: reference_diff_removed_line_number_bg(),
            content_bg: reference_diff_removed_bg(),
        },
        _ => DiffRowPalette {
            gutter_bg: diff_context_background(theme),
            content_bg: diff_panel_background(theme),
        },
    }
}

fn diff_panel_background(theme: &Theme) -> Color {
    theme.surface.panel
}

fn diff_context_background(theme: &Theme) -> Color {
    theme.surface.panel
}

fn diff_hunk_palette(theme: &Theme) -> DiffRowPalette {
    DiffRowPalette {
        gutter_bg: diff_context_background(theme),
        content_bg: diff_context_background(theme),
    }
}

fn reference_diff_added_bg() -> Color {
    Color::Rgb(0x20, 0x30, 0x3B)
}

fn reference_diff_removed_bg() -> Color {
    Color::Rgb(0x37, 0x22, 0x2C)
}

fn reference_diff_added_line_number_bg() -> Color {
    Color::Rgb(0x1B, 0x2B, 0x34)
}

fn reference_diff_removed_line_number_bg() -> Color {
    Color::Rgb(0x2D, 0x1F, 0x26)
}

fn reference_diff_highlight_added() -> Color {
    Color::Rgb(0xB8, 0xDB, 0x87)
}

fn reference_diff_highlight_removed() -> Color {
    Color::Rgb(0xE2, 0x6A, 0x75)
}

fn reference_diff_hunk_header() -> Color {
    Color::Rgb(0x82, 0x8B, 0xB8)
}

fn apply_optional_bg(style: Style, background: Option<Color>) -> Style {
    background.map_or(style, |bg| style.bg(bg))
}

fn parse_hunk_start_lines(header: &str) -> (usize, usize) {
    let mut parts = header.trim().trim_matches('@').split_whitespace();
    let before = parts.next().unwrap_or("-0");
    let after = parts.next().unwrap_or("+0");
    (
        parse_hunk_range_start(before),
        parse_hunk_range_start(after),
    )
}

fn parse_hunk_range_start(segment: &str) -> usize {
    segment
        .trim_start_matches(['-', '+'])
        .split(',')
        .next()
        .unwrap_or("0")
        .parse::<usize>()
        .unwrap_or(0)
}

fn visible_diff_line_number(line: usize) -> Option<usize> {
    (line > 0).then_some(line)
}

fn parse_unified_diff_files(
    diff_content: &str,
    fallback_path: Option<&str>,
) -> Option<Vec<ParsedPatchFile>> {
    let lines = diff_content
        .lines()
        .map(normalize_diff_line)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return None;
    }

    let mut files = Vec::new();
    let mut cursor = 0usize;

    while cursor < lines.len() {
        if !lines[cursor].starts_with("--- ") {
            cursor += 1;
            continue;
        }
        let before_label = lines[cursor].trim_start_matches("--- ").to_string();
        cursor += 1;
        if cursor >= lines.len() || !lines[cursor].starts_with("+++ ") {
            return None;
        }
        let after_label = lines[cursor].trim_start_matches("+++ ").to_string();
        let display_path = fallback_path
            .map(str::to_string)
            .or_else(|| normalize_patch_label(&after_label))
            .or_else(|| normalize_patch_label(&before_label))
            .unwrap_or_else(|| "diff".to_string());
        cursor += 1;

        let mut hunks = Vec::new();
        while cursor < lines.len() && !lines[cursor].starts_with("--- ") {
            if !lines[cursor].starts_with("@@") {
                cursor += 1;
                continue;
            }
            let header = lines[cursor].to_string();
            cursor += 1;
            let mut before_lines = Vec::new();
            let mut after_lines = Vec::new();

            while cursor < lines.len()
                && !lines[cursor].starts_with("@@")
                && !lines[cursor].starts_with("--- ")
            {
                if lines[cursor].starts_with("\\ No newline at end of file") {
                    cursor += 1;
                    continue;
                }
                let (prefix, body) = lines[cursor].split_at(1);
                match prefix {
                    " " => {
                        before_lines.push(body.to_string());
                        after_lines.push(body.to_string());
                    }
                    "-" => before_lines.push(body.to_string()),
                    "+" => after_lines.push(body.to_string()),
                    _ => return None,
                }
                cursor += 1;
            }

            hunks.push(ParsedPatchHunk {
                header,
                before_lines,
                after_lines,
            });
        }

        if !hunks.is_empty() {
            files.push(ParsedPatchFile {
                display_path,
                before_label,
                after_label,
                hunks,
            });
        }
    }

    if files.is_empty() {
        parse_hunk_only_diff(&lines, fallback_path).map(|file| vec![file])
    } else {
        Some(files)
    }
}

fn parse_hunk_only_diff(lines: &[&str], fallback_path: Option<&str>) -> Option<ParsedPatchFile> {
    let mut cursor = 0usize;
    let mut hunks = Vec::new();

    while cursor < lines.len() {
        if !lines[cursor].starts_with("@@") {
            cursor += 1;
            continue;
        }
        let header = lines[cursor].to_string();
        cursor += 1;
        let mut before_lines = Vec::new();
        let mut after_lines = Vec::new();

        while cursor < lines.len() && !lines[cursor].starts_with("@@") {
            if lines[cursor].starts_with("\\ No newline at end of file") {
                cursor += 1;
                continue;
            }
            let (prefix, body) = lines[cursor].split_at(1);
            match prefix {
                " " => {
                    before_lines.push(body.to_string());
                    after_lines.push(body.to_string());
                }
                "-" => before_lines.push(body.to_string()),
                "+" => after_lines.push(body.to_string()),
                _ => return None,
            }
            cursor += 1;
        }

        hunks.push(ParsedPatchHunk {
            header,
            before_lines,
            after_lines,
        });
    }

    (!hunks.is_empty()).then(|| ParsedPatchFile {
        display_path: fallback_path.unwrap_or("diff").to_string(),
        before_label: fallback_path.unwrap_or("diff").to_string(),
        after_label: fallback_path.unwrap_or("diff").to_string(),
        hunks,
    })
}

fn normalize_patch_label(label: &str) -> Option<String> {
    let trimmed = label
        .split_whitespace()
        .next()
        .unwrap_or(label)
        .trim_start_matches("a/")
        .trim_start_matches("b/");
    (!trimmed.is_empty() && trimmed != "/dev/null").then(|| trimmed.to_string())
}

fn normalize_diff_line(line: &str) -> &str {
    line.strip_suffix('\r').unwrap_or(line)
}

#[cfg(test)]
fn line_to_plain_text(line: Line<'static>) -> String {
    line.spans
        .into_iter()
        .map(|span| span.content.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structured_diff_rows_respect_display_width_for_wide_glyphs() {
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,2 +1,2 @@\n-漢字🙂漢字🙂漢字🙂\n+🙂漢字🙂漢字🙂漢字\n";
        let lines = render_structured_diff_lines(diff, None, "", 24, false, &Theme::default())
            .expect("wide glyph diff lines");

        assert!(
            lines.iter().all(|line| line.width() <= 24),
            "rendered diff rows should honor visible width: {:#?}",
            lines
                .iter()
                .map(|line| line_to_plain_text(line.clone()))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn stacked_diff_text_spans_keep_row_backgrounds() {
        let diff = "--- demo.txt\n+++ demo.txt\n@@ -1,3 +1,3 @@\n alpha\n-beta\n+BETA\n gamma\n";
        let theme = Theme::default();
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            80,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &theme,
        )
        .expect("stacked diff lines");

        let context_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("alpha"))
            .expect("context row");
        let removed_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("beta"))
            .expect("removed row");
        let added_line = lines
            .iter()
            .find(|line| line_to_plain_text((*line).clone()).contains("BETA"))
            .expect("added row");

        let context_span = context_line
            .spans
            .iter()
            .find(|span| span.content.contains("alpha"))
            .expect("context text span");
        let removed_span = removed_line
            .spans
            .iter()
            .find(|span| span.content.contains("beta"))
            .expect("removed text span");
        let added_span = added_line
            .spans
            .iter()
            .find(|span| span.content.contains("BETA"))
            .expect("added text span");

        assert_eq!(context_span.style.bg, Some(theme.surface.panel));
        assert_eq!(
            removed_span.style.bg,
            Some(diff_row_palette('-', &theme).content_bg)
        );
        assert_eq!(
            added_span.style.bg,
            Some(diff_row_palette('+', &theme).content_bg)
        );
    }

    #[test]
    fn structured_diff_palette_matches_reference_inline_diff_colors() {
        let theme = Theme::default();

        assert_eq!(
            diff_row_palette('+', &theme).content_bg,
            reference_diff_added_bg()
        );
        assert_eq!(
            diff_row_palette('+', &theme).gutter_bg,
            reference_diff_added_line_number_bg()
        );
        assert_eq!(
            diff_row_palette('-', &theme).content_bg,
            reference_diff_removed_bg()
        );
        assert_eq!(
            diff_row_palette('-', &theme).gutter_bg,
            reference_diff_removed_line_number_bg()
        );
        assert_eq!(diff_hunk_palette(&theme).content_bg, theme.surface.panel);
        assert_eq!(
            diff_marker_style('+', None, &theme).fg,
            Some(reference_diff_highlight_added())
        );
        assert_eq!(
            diff_marker_style('-', None, &theme).fg,
            Some(reference_diff_highlight_removed())
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Added,
                DiffSegmentKind::Removed,
                None,
                &theme
            )
            .fg,
            Some(reference_diff_highlight_added())
        );
        assert_eq!(
            diff_segment_style(
                DiffSegmentKind::Removed,
                DiffSegmentKind::Added,
                None,
                &theme
            )
            .fg,
            Some(reference_diff_highlight_removed())
        );

        let hunk_header = render_diff_hunk_header("", "@@ -1,1 +1,1 @@", 48, 2, &theme);
        let hunk_span = hunk_header
            .spans
            .iter()
            .find(|span| span.content.contains("@@ -1,1 +1,1 @@"))
            .expect("hunk header span");
        assert_eq!(hunk_span.style.fg, Some(reference_diff_hunk_header()));
        assert_eq!(hunk_span.style.bg, Some(theme.surface.panel));
    }

    #[test]
    fn structured_diff_syntax_highlighting_uses_reference_token_colors() {
        let chunks = highlight_diff_line_chunks(
            Some("src/demo.rs"),
            "let value = \"hi\"; let total = 42; // note",
            Some(reference_diff_added_bg()),
        )
        .expect("syntax-highlighted diff chunks");

        let find_chunk = |needle: &str| {
            chunks
                .iter()
                .find(|chunk| chunk.text.contains(needle))
                .unwrap_or_else(|| panic!("missing chunk containing {needle:?}: {chunks:#?}"))
        };

        assert_eq!(
            find_chunk("hi").style.fg,
            Some(Color::Rgb(0x7F, 0xD8, 0x8F))
        );
        assert_eq!(
            find_chunk("42").style.fg,
            Some(Color::Rgb(0xF5, 0xA7, 0x42))
        );
        assert_eq!(
            find_chunk("note").style.fg,
            Some(Color::Rgb(0x80, 0x80, 0x80))
        );
        assert_eq!(find_chunk("note").style.bg, Some(reference_diff_added_bg()));
    }

    #[test]
    fn structured_diff_headers_surface_rename_paths() {
        let diff = "--- src/old_name.rs\n+++ src/new_name.rs\n@@ -1,1 +1,1 @@\n-old\n+new\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            96,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .expect("rename diff lines");
        let header = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .find(|line| line.contains("old_name.rs") && line.contains("new_name.rs"))
            .expect("rename header");

        assert!(
            header.contains("→"),
            "header should surface rename arrow: {header}"
        );
    }

    #[test]
    fn stacked_diff_long_rows_wrap_instead_of_truncating() {
        let diff = "--- docs/transcript.md\n+++ docs/transcript.md\n@@ -1,1 +1,1 @@\n-session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows\n+session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells\n";
        let lines = render_structured_diff_lines_with_options(
            diff,
            None,
            "",
            84,
            StructuredDiffRenderOptions {
                force_stacked: true,
                highlight_intraline: false,
                highlight_syntax: false,
                show_file_header: true,
                show_hunk_header: true,
            },
            &Theme::default(),
        )
        .expect("wrapped stacked diff lines");
        let rendered = lines
            .iter()
            .map(|line| line_to_plain_text(line.clone()))
            .collect::<Vec<_>>();
        let collect_stacked_cell_text = |rows: &[String], marker: char| {
            let marker_token = format!("{marker} ");
            let start = rows
                .iter()
                .position(|line| line.contains(&marker_token))
                .unwrap_or_else(|| panic!("missing {marker} row marker: {rows:#?}"));
            let marker_column = rows[start]
                .find(&marker_token)
                .unwrap_or_else(|| panic!("missing {marker} marker column: {rows:#?}"));
            let text_column = marker_column + marker_token.len();
            let mut chunks = Vec::new();

            for line in &rows[start..] {
                let marker_cell = line.get(marker_column..text_column).unwrap_or("");
                let Some(text) = line.get(text_column..) else {
                    break;
                };
                let text = text.trim_end();

                if marker_cell == marker_token {
                    chunks.push(text.to_string());
                    continue;
                }

                if marker_cell == "  " && !text.is_empty() {
                    chunks.push(text.to_string());
                    continue;
                }

                break;
            }

            chunks.concat()
        };
        let removed_text = collect_stacked_cell_text(&rendered, '-');
        let added_text = collect_stacked_cell_text(&rendered, '+');

        assert!(
            removed_text
                == "session turn diff view keeps the tool row spacing perfectly aligned in every transcript lane for operators reviewing compact windows",
            "removed continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            added_text
                == "session turn diff view keeps the tool row spacing perfectly aligned across the transcript surface for operators reviewing compact windows and narrow shells",
            "added continuation should preserve the full text across wrapped rows: {rendered:#?}"
        );
        assert!(
            rendered.iter().all(|line| !line.contains('…')),
            "stacked renderer should keep the full text without ellipsis: {rendered:#?}"
        );
    }
}
