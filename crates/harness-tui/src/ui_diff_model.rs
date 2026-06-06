use imara_diff::{Algorithm, Diff, InternedInput};
use std::cmp::max;

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
pub(super) struct StructuredDiffModel {
    pub(super) files: Vec<StructuredDiffFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StructuredDiffFile {
    pub(super) display_path: String,
    pub(super) before_path: Option<String>,
    pub(super) after_path: Option<String>,
    pub(super) additions: usize,
    pub(super) removals: usize,
    pub(super) rows: Vec<StructuredDiffDisplayRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StructuredDiffDisplayRow {
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
pub(super) struct DiffCell {
    pub(super) marker: char,
    pub(super) line_number: Option<usize>,
    pub(super) text: String,
    pub(super) segments: Vec<DiffSegment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DiffSegment {
    pub(super) kind: DiffSegmentKind,
    pub(super) text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DiffSegmentKind {
    Unchanged,
    Removed,
    Added,
}

pub(super) fn structured_diff_model_from_patch(
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

pub(super) fn structured_diff_stats_from_patch(
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
    pub(crate) rows: Vec<StructuredDiffDisplayRow>,
    pub(crate) additions: usize,
    pub(crate) removals: usize,
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
