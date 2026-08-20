use std::path::Path;

use crate::parity::{IdentityMaskRegistry, SemanticCell, SemanticFrame};
use crate::tui_fidelity::{
    CheckpointName, Scenario, TextPlacement, TextStyle, TextSubstitution, Wrapping,
};
use crate::tui_fidelity_runner::{AdapterReceipt, DualRuntimeReceipt};

use super::super::error::ComparatorError;
use super::super::types::CellSnapshot;
use super::runtime_pair;

pub fn semantic(scenario: &Scenario, capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    let mut errors = Vec::new();
    for checkpoint in required_checkpoints() {
        let expected = read_snapshot(reference, checkpoint)?;
        let actual = read_snapshot(candidate, checkpoint)?;
        let masks = verified_masks_for(scenario, checkpoint, &expected.frame, &actual.frame)?;
        if let Err(error) = super::super::cells::compare_cells(&expected, &actual, &masks) {
            errors.push(error.to_string());
        }
    }
    finish(errors)
}

pub fn pixels(scenario: &Scenario, capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    let mut errors = Vec::new();
    for checkpoint in required_checkpoints() {
        let reference_png = artifact_path(reference, checkpoint, "terminal.png")?;
        let candidate_png = artifact_path(candidate, checkpoint, "terminal.png")?;
        let reference_bytes = read_file(&reference_png)?;
        let candidate_bytes = read_file(&candidate_png)?;
        let reference_cells = read_snapshot(reference, checkpoint)?;
        let candidate_cells = read_snapshot(candidate, checkpoint)?;
        verify_substitution_values(
            scenario,
            checkpoint,
            &reference_cells.frame,
            &candidate_cells.frame,
        )?;
        let spans = pixel_spans(scenario, checkpoint, &reference_bytes)?;
        if let Err(error) =
            super::super::pixels::compare_png_bytes(&reference_bytes, &candidate_bytes, &spans)
        {
            errors.push(error.to_string());
        }
    }
    finish(errors)
}

pub fn artifact_path(
    runtime: &AdapterReceipt,
    checkpoint: CheckpointName,
    name: &str,
) -> Result<std::path::PathBuf, ComparatorError> {
    runtime
        .checkpoints
        .iter()
        .find(|item| item.name == checkpoint)
        .and_then(|item| {
            item.artifacts
                .iter()
                .find(|artifact| artifact.path.ends_with(name))
        })
        .map(|artifact| std::path::PathBuf::from(&artifact.path))
        .ok_or_else(|| ComparatorError::Invalid {
            detail: format!(
                "{} {} artifact {name} is missing",
                runtime.adapter.as_str(),
                checkpoint.as_str()
            ),
        })
}

fn required_checkpoints() -> [CheckpointName; 3] {
    [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ]
}

fn finish(errors: Vec<String>) -> Result<(), ComparatorError> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ComparatorError::Invalid {
            detail: errors.join("; "),
        })
    }
}

fn read_snapshot(
    runtime: &AdapterReceipt,
    checkpoint: CheckpointName,
) -> Result<CellSnapshot, ComparatorError> {
    let path = artifact_path(runtime, checkpoint, "cells.json")?;
    let frame = SemanticFrame::read_cells_json(&path).map_err(|error| ComparatorError::Io {
        path: path.clone(),
        detail: error.to_string(),
    })?;
    Ok(CellSnapshot {
        frame,
        focus: None,
        z_order: Vec::new(),
    })
}

pub(super) fn verified_masks_for(
    scenario: &Scenario,
    checkpoint: CheckpointName,
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
) -> Result<IdentityMaskRegistry, ComparatorError> {
    verify_substitution_values(scenario, checkpoint, reference, candidate)?;
    Ok(scenario
        .substitutions
        .iter()
        .filter(|item| item.checkpoint == checkpoint)
        .filter_map(|substitution| {
            let rectangle = clipped_rectangle(substitution.rectangle, scenario, checkpoint)?;
            let cells = (rectangle.row..rectangle.row.saturating_add(rectangle.rows))
                .flat_map(|row| {
                    (rectangle.col..rectangle.col.saturating_add(rectangle.cols))
                        .map(move |col| (row, col))
                })
                .collect::<Vec<_>>();
            Some((substitution.field.mask_label(substitution.kind), cells))
        })
        .fold(IdentityMaskRegistry::new(), |masks, (field, cells)| {
            masks.with_field(field, cells)
        }))
}

fn verify_substitution_values(
    scenario: &Scenario,
    checkpoint: CheckpointName,
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
) -> Result<(), ComparatorError> {
    for substitution in scenario
        .substitutions
        .iter()
        .filter(|item| item.checkpoint == checkpoint)
    {
        verify_placement(
            reference,
            substitution,
            &substitution.reference,
            "reference",
        )?;
        verify_placement(
            candidate,
            substitution,
            &substitution.candidate,
            "candidate",
        )?;
    }
    Ok(())
}

fn verify_placement(
    frame: &SemanticFrame,
    substitution: &TextSubstitution,
    placement: &TextPlacement,
    side: &str,
) -> Result<(), ComparatorError> {
    let lines = match placement.wrapping {
        Wrapping::NoWrap => vec![placement.text.as_str()],
        Wrapping::HardWrap => placement.text.split('\n').collect(),
    };
    if lines.len() != usize::from(substitution.rectangle.rows) {
        return placement_error(
            substitution,
            side,
            "declared line count does not match rectangle",
        );
    }
    for (row_offset, declared_line) in lines.into_iter().enumerate() {
        let row = substitution
            .rectangle
            .row
            .saturating_add(
                u16::try_from(row_offset).map_err(|_| ComparatorError::Invalid {
                    detail: "substitution row offset exceeds u16".to_owned(),
                })?,
            );
        let content_start = substitution
            .rectangle
            .col
            .saturating_add(placement.padding_left);
        let content_end = content_start.saturating_add(placement.cell_width);
        let mut observed_line = String::new();
        for col in substitution.rectangle.col
            ..substitution
                .rectangle
                .col
                .saturating_add(substitution.rectangle.cols)
        {
            let cell = frame
                .cell(row, col)
                .ok_or_else(|| ComparatorError::Invalid {
                    detail: format!(
                        "{} {} substitution rectangle is outside the {side} frame",
                        substitution.kind.as_str(),
                        substitution.field.as_str()
                    ),
                })?;
            verify_style(cell, placement.style, substitution, side)?;
            if col < content_start || col >= content_end {
                if !cell.grapheme.is_empty() || cell.continuation {
                    return placement_error(substitution, side, "captured padding is not blank");
                }
            } else if !cell.continuation {
                if cell.grapheme.is_empty() {
                    observed_line.push(' ');
                } else {
                    observed_line.push_str(&cell.grapheme);
                }
            }
        }
        if observed_line != declared_line {
            return placement_error(
                substitution,
                side,
                &format!(
                    "captured text {observed_line:?} does not equal declared value {declared_line:?}"
                ),
            );
        }
    }
    Ok(())
}

fn verify_style(
    cell: &SemanticCell,
    style: TextStyle,
    substitution: &TextSubstitution,
    side: &str,
) -> Result<(), ComparatorError> {
    let matches = cell.fg.r == style.foreground.r
        && cell.fg.g == style.foreground.g
        && cell.fg.b == style.foreground.b
        && cell.bg.r == style.background.r
        && cell.bg.g == style.background.g
        && cell.bg.b == style.background.b
        && cell.modifiers.bold == style.bold
        && cell.modifiers.dim == style.dim
        && cell.modifiers.italic == style.italic
        && cell.modifiers.underline == style.underline
        && cell.modifiers.inverse == style.inverse
        && cell.hyperlink.is_none();
    if matches {
        Ok(())
    } else {
        placement_error(
            substitution,
            side,
            "captured style does not equal declared style",
        )
    }
}

fn placement_error<T>(
    substitution: &TextSubstitution,
    side: &str,
    detail: &str,
) -> Result<T, ComparatorError> {
    Err(ComparatorError::Invalid {
        detail: format!(
            "{} {} {side} placement: {detail}",
            substitution.kind.as_str(),
            substitution.field.as_str()
        ),
    })
}

fn pixel_spans(
    scenario: &Scenario,
    checkpoint: CheckpointName,
    png: &[u8],
) -> Result<Vec<super::super::pixels::IdentityPixelSpan>, ComparatorError> {
    let image =
        image::load_from_memory_with_format(png, image::ImageFormat::Png).map_err(|error| {
            ComparatorError::PngDecode {
                side: "reference".to_owned(),
                detail: error.to_string(),
            }
        })?;
    let viewport = scenario
        .checkpoints
        .iter()
        .find(|item| item.name == checkpoint)
        .map(|item| item.frame.viewport)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "scenario checkpoint is missing".to_owned(),
        })?;
    let cell_width = image.width() / u32::from(viewport.cols);
    let cell_height = image.height() / u32::from(viewport.rows);
    if cell_width == 0 || cell_height == 0 {
        return Err(ComparatorError::Invalid {
            detail: "PNG is smaller than the scenario viewport".to_owned(),
        });
    }
    Ok(scenario
        .substitutions
        .iter()
        .filter(|item| item.checkpoint == checkpoint)
        .filter_map(|item: &TextSubstitution| {
            let rectangle = clipped_rectangle(item.rectangle, scenario, checkpoint)?;
            Some(super::super::pixels::IdentityPixelSpan::from_cell_rect(
                &item.field.mask_label(item.kind),
                rectangle,
                image.width(),
                image.height(),
                viewport.cols,
                viewport.rows,
            ))
        })
        .collect())
}

fn clipped_rectangle(
    rectangle: crate::tui_fidelity::CellRect,
    scenario: &Scenario,
    checkpoint: CheckpointName,
) -> Option<crate::tui_fidelity::CellRect> {
    let viewport = scenario
        .checkpoints
        .iter()
        .find(|item| item.name == checkpoint)
        .map(|item| item.frame.viewport)?;
    let col_end = u32::from(rectangle.col)
        .saturating_add(u32::from(rectangle.cols))
        .min(u32::from(viewport.cols));
    let row_end = u32::from(rectangle.row)
        .saturating_add(u32::from(rectangle.rows))
        .min(u32::from(viewport.rows));
    let col_start = u32::from(rectangle.col).min(u32::from(viewport.cols));
    let row_start = u32::from(rectangle.row).min(u32::from(viewport.rows));
    if col_start >= col_end || row_start >= row_end {
        return None;
    }
    Some(crate::tui_fidelity::CellRect {
        col: u16::try_from(col_start).ok()?,
        row: u16::try_from(row_start).ok()?,
        cols: u16::try_from(col_end - col_start).ok()?,
        rows: u16::try_from(row_end - row_start).ok()?,
    })
}

fn read_file(path: &Path) -> Result<Vec<u8>, ComparatorError> {
    std::fs::read(path).map_err(|error| ComparatorError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}
