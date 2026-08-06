use std::collections::BTreeSet;

use crate::parity::{frame_line, CursorState, SemanticCell, SemanticFrame};
use crate::tui_fidelity::CellRect;

use super::pixels::IdentityPixelSpan;

pub(super) fn dynamic_identity_cells(
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
) -> BTreeSet<(u16, u16)> {
    if reference.cols != candidate.cols || reference.rows != candidate.rows {
        return BTreeSet::new();
    }

    let mut cells = BTreeSet::new();
    for row in 0..reference.rows {
        let differing = (0..reference.cols)
            .filter(|&col| {
                let Some(reference_cell) = reference.cell(row, col) else {
                    return false;
                };
                let Some(candidate_cell) = candidate.cell(row, col) else {
                    return false;
                };
                reference_cell.grapheme != candidate_cell.grapheme
                    && same_non_text_fields(reference_cell, candidate_cell)
            })
            .collect::<Vec<_>>();

        for run in contiguous_runs(&differing) {
            if plausible_identity_run(reference, candidate, row, run.0, run.1) {
                for col in run.0..=run.1 {
                    cells.insert((row, col));
                }
            }
        }
    }
    cells
}

pub(super) fn dynamic_identity_pixel_spans(
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
    width: u32,
    height: u32,
) -> Vec<IdentityPixelSpan> {
    let cell_width = width.checked_div(u32::from(reference.cols)).unwrap_or(0);
    let cell_height = height.checked_div(u32::from(reference.rows)).unwrap_or(0);
    if cell_width == 0 || cell_height == 0 {
        return Vec::new();
    }

    dynamic_identity_cells(reference, candidate)
        .into_iter()
        .map(|(row, col)| {
            IdentityPixelSpan::from_cell_rect(
                "dynamic_identity",
                CellRect {
                    col,
                    row,
                    cols: 1,
                    rows: 1,
                },
                cell_width,
                cell_height,
            )
        })
        .collect()
}

fn same_non_text_fields(reference: &SemanticCell, candidate: &SemanticCell) -> bool {
    reference.row == candidate.row
        && reference.col == candidate.col
        && reference.width == candidate.width
        && reference.continuation == candidate.continuation
        && reference.fg == candidate.fg
        && reference.bg == candidate.bg
        && reference.modifiers == candidate.modifiers
        && reference.hyperlink == candidate.hyperlink
}

fn contiguous_runs(columns: &[u16]) -> Vec<(u16, u16)> {
    let mut runs = Vec::new();
    let Some(&first) = columns.first() else {
        return runs;
    };
    let mut start = first;
    let mut end = first;
    for &column in &columns[1..] {
        if column == end.saturating_add(1) {
            end = column;
        } else {
            runs.push((start, end));
            start = column;
            end = column;
        }
    }
    runs.push((start, end));
    runs
}

fn plausible_identity_run(
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
    row: u16,
    start: u16,
    end: u16,
) -> bool {
    let reference_text = run_text(reference, row, start, end);
    let candidate_text = run_text(candidate, row, start, end);
    let combined = format!("{reference_text} {candidate_text}");

    contains_braille(&combined)
        || contains_path_marker(&combined)
        || contains_version(&combined)
        || bounded_identity_name(&combined)
        || provider_status_context(reference, candidate, row, start, end)
}

fn run_text(frame: &SemanticFrame, row: u16, start: u16, end: u16) -> String {
    (start..=end)
        .filter_map(|col| frame.cell(row, col))
        .filter(|cell| !cell.continuation)
        .map(|cell| cell.grapheme.as_str())
        .collect()
}

fn contains_braille(text: &str) -> bool {
    text.chars()
        .any(|character| ('\u{2800}'..='\u{28ff}').contains(&character))
}

fn contains_path_marker(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || text.contains("~")
}

fn contains_version(text: &str) -> bool {
    let substitution = crate::parity::IdentitySubstitution::new().with_version();
    !substitution.normalize_detailed(text).1.is_empty()
}

fn bounded_identity_name(text: &str) -> bool {
    if text.chars().count() > 24 || !text.chars().any(char::is_uppercase) {
        return false;
    }
    [
        "Grok", "Harness", "Codex", "OpenAI", "XAI", "Luna", "Demo", "Beta",
    ]
    .iter()
    .any(|name| text.contains(name))
}

fn provider_status_context(
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
    row: u16,
    start: u16,
    end: u16,
) -> bool {
    if row.saturating_add(5) < reference.rows {
        return false;
    }
    let line = format!(
        "{} {}",
        frame_line(reference, row),
        frame_line(candidate, row)
    );
    let has_status_marker = [
        "CLIProxy",
        "always-approve",
        "model-",
        "GPT",
        "provider",
        "account",
    ]
    .iter()
    .any(|marker| line.contains(marker));
    has_status_marker && run_text(reference, row, start, end).chars().count() <= 64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_path_and_logo_text_but_rejects_copy_and_style_drift() {
        let reference = frame(&[
            (0, "~/reference/project", false),
            (1, "\u{2800}\u{28ff}", false),
            (2, "New reference copy", false),
        ]);
        let mut candidate = frame(&[
            (0, "codex/project", false),
            (1, "\u{28c0}\u{2801}", false),
            (2, "New candidate copy", false),
        ]);

        let cells = dynamic_identity_cells(&reference, &candidate);

        assert!(cells.contains(&(0, 0)));
        assert!(cells.contains(&(1, 0)));
        assert!(!cells.contains(&(2, 0)));

        candidate.cell_mut(0, 0).expect("path cell").fg =
            crate::parity::ResolvedRgb::new(255, 0, 0);
        assert!(!dynamic_identity_cells(&reference, &candidate).contains(&(0, 0)));
    }

    fn frame(lines: &[(u16, &str, bool)]) -> SemanticFrame {
        let mut frame = SemanticFrame::new(32, 3, CursorState::hidden(0, 0));
        for (row, text, _) in lines {
            for (col, grapheme) in text.chars().enumerate() {
                let col = u16::try_from(col).expect("fixture fits");
                frame
                    .set_cell(SemanticCell::blank(*row, col).with_grapheme(grapheme.to_string(), 1))
                    .expect("fixture cell");
            }
        }
        frame
    }
}
