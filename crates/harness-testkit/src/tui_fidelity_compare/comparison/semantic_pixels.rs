use std::path::Path;

use crate::parity::{IdentityMaskRegistry, SemanticFrame};
use crate::tui_fidelity::{CheckpointName, IdentitySubstitution, Scenario};
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
        let masks = masks_for_frames(scenario, checkpoint, &expected.frame, &actual.frame);
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
        let reference_frame = read_snapshot(reference, checkpoint)?.frame;
        let candidate_frame = read_snapshot(candidate, checkpoint)?.frame;
        let mut spans = pixel_spans(scenario, checkpoint, &reference_bytes)?;
        let image = image::load_from_memory_with_format(&reference_bytes, image::ImageFormat::Png)
            .map_err(|error| ComparatorError::PngDecode {
                side: "reference".to_owned(),
                detail: error.to_string(),
            })?;
        spans.extend(super::super::dynamic::dynamic_identity_pixel_spans(
            &reference_frame,
            &candidate_frame,
            image.width(),
            image.height(),
        ));
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

fn masks_for(scenario: &Scenario, checkpoint: CheckpointName) -> IdentityMaskRegistry {
    scenario
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
            Some((substitution.scope.placeholder(), cells))
        })
        .fold(IdentityMaskRegistry::new(), |masks, (field, cells)| {
            masks.with_field(field, cells)
        })
}

fn masks_for_frames(
    scenario: &Scenario,
    checkpoint: CheckpointName,
    reference: &SemanticFrame,
    candidate: &SemanticFrame,
) -> IdentityMaskRegistry {
    let dynamic_cells = super::super::dynamic::dynamic_identity_cells(reference, candidate);
    if dynamic_cells.is_empty() {
        masks_for(scenario, checkpoint)
    } else {
        masks_for(scenario, checkpoint).with_field("dynamic_identity", dynamic_cells)
    }
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
        .filter_map(|item: &IdentitySubstitution| {
            let rectangle = clipped_rectangle(item.rectangle, scenario, checkpoint)?;
            Some(super::super::pixels::IdentityPixelSpan::from_cell_rect(
                item.scope.placeholder(),
                rectangle,
                cell_width,
                cell_height,
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
