use crate::parity::{
    CellModifiers, ResolvedRgb, SemanticCell, SemanticFrame, DEFAULT_BG, DEFAULT_FG,
};
use crate::tui_fidelity::{CheckpointName, KeyCode, Scenario, ScenarioAction};
use crate::tui_fidelity_runner::{AdapterReceipt, DualRuntimeReceipt};

use super::super::error::ComparatorError;
use super::runtime_pair;
use super::semantic_pixels::artifact_path;

#[derive(Clone, Debug, PartialEq, Eq)]
enum TranscriptToken {
    Gap,
    Cell {
        grapheme: String,
        width: u8,
        continuation: bool,
        fg: ResolvedRgb,
        bg: ResolvedRgb,
        modifiers: CellModifiers,
        hyperlink: Option<String>,
    },
}

pub(super) fn compare(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    if !submits_prompt(scenario) {
        return Ok(());
    }
    let (reference, candidate) = runtime_pair(capture)?;
    for checkpoint in [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ] {
        let reference_path = artifact_path(reference, checkpoint, "cells.json")?;
        let candidate_path = artifact_path(candidate, checkpoint, "cells.json")?;
        let reference_frame = read_frame(&reference_path)?;
        let candidate_frame = read_frame(&candidate_path)?;
        let expected = canonical_tokens(&reference_frame)?;
        let actual = canonical_tokens(&candidate_frame)?;
        if expected != actual {
            let index = expected
                .iter()
                .zip(&actual)
                .position(|(left, right)| left != right)
                .unwrap_or(expected.len().min(actual.len()));
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "{} transcript grammar differs at token {index}: reference={:?}, candidate={:?}",
                    checkpoint.as_str(),
                    expected.get(index),
                    actual.get(index)
                ),
            });
        }
    }
    Ok(())
}

pub(super) fn compare_motion(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    if !submits_prompt(scenario) {
        return Ok(());
    }
    let (reference, candidate) = runtime_pair(capture)?;
    let expected = ordered_states(reference)?;
    let actual = ordered_states(candidate)?;
    if expected != actual {
        return Err(ComparatorError::Invalid {
            detail: "ordered transcript checkpoint states differ".to_owned(),
        });
    }
    Ok(())
}

fn submits_prompt(scenario: &Scenario) -> bool {
    scenario.actions.iter().any(|action| {
        matches!(
            action,
            ScenarioAction::TimedKey(key) if key.key.code == KeyCode::Enter
        )
    })
}

fn ordered_states(
    runtime: &AdapterReceipt,
) -> Result<Vec<(CheckpointName, Vec<TranscriptToken>)>, ComparatorError> {
    let expected_names = [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ];
    if runtime.checkpoints.len() != expected_names.len()
        || !runtime
            .checkpoints
            .iter()
            .map(|checkpoint| checkpoint.name)
            .eq(expected_names)
    {
        return Err(ComparatorError::Invalid {
            detail: format!(
                "{} transcript checkpoints are not ordered rest, mid, settled",
                runtime.adapter.as_str()
            ),
        });
    }
    if runtime
        .checkpoints
        .windows(2)
        .any(|pair| pair[0].captured_at_millis >= pair[1].captured_at_millis)
    {
        return Err(ComparatorError::Invalid {
            detail: format!(
                "{} transcript checkpoint timestamps are not strictly increasing",
                runtime.adapter.as_str()
            ),
        });
    }
    runtime
        .checkpoints
        .iter()
        .map(|checkpoint| {
            let path = artifact_path(runtime, checkpoint.name, "cells.json")?;
            let frame = read_frame(&path)?;
            Ok((checkpoint.name, canonical_tokens(&frame)?))
        })
        .collect()
}

fn read_frame(path: &std::path::Path) -> Result<SemanticFrame, ComparatorError> {
    SemanticFrame::read_cells_json(path).map_err(|error| ComparatorError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn canonical_tokens(frame: &SemanticFrame) -> Result<Vec<TranscriptToken>, ComparatorError> {
    let composer_row = (0..frame.rows)
        .find(|&row| row_text(frame, row).contains('╭'))
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "transcript grammar frame is missing the composer border".to_owned(),
        })?;
    let start_row = (0..composer_row)
        .find(|&row| row_cells(frame, row).any(|cell| cell.grapheme == "❯"))
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "transcript grammar frame is missing the submitted prompt".to_owned(),
        })?;

    let mut tokens = Vec::new();
    for row in start_row..composer_row {
        let text = row_text(frame, row);
        if shell_status_row(text.trim()) {
            continue;
        }
        let limit = timestamp_start(&text, frame.cols).unwrap_or(frame.cols);
        let cells = row_cells(frame, row)
            .take(usize::from(limit))
            .filter(|cell| cell_has_content(cell))
            .collect::<Vec<_>>();
        if cells.is_empty() {
            continue;
        }
        if !tokens.is_empty() && !matches!(tokens.last(), Some(TranscriptToken::Gap)) {
            tokens.push(TranscriptToken::Gap);
        }
        let mut prior_col = None;
        for cell in cells {
            if prior_col.is_some_and(|col| cell.col > col + 1)
                && !matches!(tokens.last(), Some(TranscriptToken::Gap))
            {
                tokens.push(TranscriptToken::Gap);
            }
            tokens.push(token(cell));
            prior_col = Some(cell.col);
        }
    }
    if tokens.is_empty() {
        return Err(ComparatorError::Invalid {
            detail: "transcript grammar frame contains no transcript tokens".to_owned(),
        });
    }
    Ok(tokens)
}

fn row_cells(frame: &SemanticFrame, row: u16) -> impl Iterator<Item = &SemanticCell> {
    frame.cells.iter().filter(move |cell| cell.row == row)
}

fn cell_has_content(cell: &SemanticCell) -> bool {
    cell.continuation
        || cell
            .grapheme
            .chars()
            .any(|grapheme| !grapheme.is_whitespace())
}

fn row_text(frame: &SemanticFrame, row: u16) -> String {
    row_cells(frame, row)
        .map(|cell| {
            if cell.continuation || cell.grapheme.is_empty() {
                ' '
            } else {
                cell.grapheme.chars().next().unwrap_or(' ')
            }
        })
        .collect()
}

fn shell_status_row(text: &str) -> bool {
    text.contains("Responding…")
        || text.starts_with("Run /doctor")
        || text.starts_with("Worked for ")
        || text.starts_with("Tight on space? Try /compact-mode")
}

fn timestamp_start(text: &str, cols: u16) -> Option<u16> {
    let trimmed = text.trim_end();
    let (prefix, meridiem) = trimmed.rsplit_once(' ')?;
    if meridiem != "AM" && meridiem != "PM" {
        return None;
    }
    let (before_time, time) = prefix.rsplit_once(' ')?;
    if !before_time.ends_with(' ') || !valid_clock(time) {
        return None;
    }
    let start = before_time.chars().count().saturating_add(1);
    let start = u16::try_from(start).ok()?;
    (start >= cols.saturating_mul(2) / 3).then_some(start)
}

fn valid_clock(value: &str) -> bool {
    let Some((hours, minutes)) = value.split_once(':') else {
        return false;
    };
    hours.len() <= 2
        && minutes.len() == 2
        && hours
            .parse::<u8>()
            .is_ok_and(|hours| (1..=12).contains(&hours))
        && minutes.parse::<u8>().is_ok_and(|minutes| minutes < 60)
}

fn token(cell: &SemanticCell) -> TranscriptToken {
    TranscriptToken::Cell {
        grapheme: cell.grapheme.clone(),
        width: cell.width,
        continuation: cell.continuation,
        fg: cell.fg,
        bg: cell.bg,
        modifiers: cell.modifiers,
        hyperlink: cell.hyperlink.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parity::{CursorState, SemanticCell};

    #[test]
    fn canonical_tokens_accept_wrapping_and_clock_suffix_differences() {
        // arrange
        let reference = frame(&[
            (1, 2, "❯"),
            (1, 4, "go"),
            (1, 30, "1:39 AM"),
            (3, 2, "done"),
            (4, 2, "now"),
            (6, 2, "Tight on space? Try /compact-mode"),
            (8, 0, "╭"),
        ]);
        let candidate = frame(&[
            (1, 2, "❯"),
            (1, 4, "go"),
            (3, 2, "done"),
            (3, 7, "now"),
            (8, 0, "╭"),
        ]);

        // act
        let reference = canonical_tokens(&reference).expect("reference tokens");
        let candidate = canonical_tokens(&candidate).expect("candidate tokens");
        // assert
        assert_eq!(reference, candidate);
    }

    #[test]
    fn canonical_tokens_reject_rail_glyph_mutation() {
        // arrange
        let reference = frame(&[(1, 2, "❯"), (3, 2, "┃ command"), (8, 0, "╭")]);
        let candidate = frame(&[(1, 2, "❯"), (3, 2, "❙ command"), (8, 0, "╭")]);

        // act
        let reference = canonical_tokens(&reference).expect("reference tokens");
        let candidate = canonical_tokens(&candidate).expect("candidate tokens");
        // assert
        assert_ne!(reference, candidate);
    }

    #[test]
    fn canonical_tokens_reject_rail_style_mutation() {
        // arrange
        let reference = frame(&[(1, 2, "❯"), (3, 2, "┃ command"), (8, 0, "╭")]);
        let mut candidate = reference.clone();
        candidate.cell_mut(3, 2).expect("rail cell").modifiers.dim = true;

        // act
        let reference = canonical_tokens(&reference).expect("reference tokens");
        let candidate = canonical_tokens(&candidate).expect("candidate tokens");
        // assert
        assert_ne!(reference, candidate);
    }

    fn frame(entries: &[(u16, u16, &str)]) -> SemanticFrame {
        let mut frame = SemanticFrame::new(40, 10, CursorState::hidden(0, 0));
        for &(row, col, text) in entries {
            for (offset, grapheme) in text.chars().enumerate() {
                let offset = u16::try_from(offset).expect("fixture offset");
                frame
                    .set_cell(
                        SemanticCell::blank(row, col + offset)
                            .with_grapheme(grapheme.to_string(), 1),
                    )
                    .expect("fixture cell");
            }
        }
        frame
    }

    #[test]
    fn default_colors_match_blank_fixture_cells() {
        // arrange
        let expected = (
            ResolvedRgb::from_array(DEFAULT_FG),
            ResolvedRgb::from_array(DEFAULT_BG),
        );
        // act
        let cell = SemanticCell::blank(0, 0);
        // assert
        assert_eq!((cell.fg, cell.bg), expected);
    }
}
