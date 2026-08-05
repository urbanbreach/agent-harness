use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::{Command, Stdio};

use crate::parity::{IdentityMaskRegistry, SemanticFrame};
use crate::tui_fidelity::{CheckpointName, IdentitySubstitution, Scenario};
use crate::tui_fidelity_runner::{AdapterReceipt, CleanupReceipt, DualRuntimeReceipt};

use super::error::ComparatorError;
use super::types::{CellSnapshot, ComparisonReceipt, GateReceipt, COMPARISON_RECEIPT_SCHEMA};

const REQUIRED_ARTIFACTS: [&str; 6] = [
    "terminal.png",
    "terminal.txt",
    "terminal-ansi.txt",
    "cells.json",
    "cells.txt",
    "metadata.json",
];

pub fn compare_capture(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
    cleanup: &CleanupReceipt,
) -> ComparisonReceipt {
    let mut gates = BTreeMap::new();
    record_gate(
        &mut gates,
        "semantic_cell",
        compare_semantic(scenario, capture),
    );
    record_gate(&mut gates, "pixel", compare_pixels(scenario, capture));
    record_gate(&mut gates, "motion", compare_motion_gate(scenario, capture));
    record_gate(&mut gates, "timing", compare_timing_gate(capture));
    record_gate(&mut gates, "provenance", compare_provenance(capture));
    record_gate(&mut gates, "checkpoint", compare_checkpoints(capture));
    record_gate(&mut gates, "exit", compare_exits(scenario, capture));
    record_gate(&mut gates, "cleanup", compare_cleanup(cleanup));
    let comparison_passed = gates.values().all(|gate| gate.passed);
    ComparisonReceipt {
        schema_version: COMPARISON_RECEIPT_SCHEMA.to_owned(),
        capture_succeeded: true,
        comparison_passed,
        gates,
    }
}

fn record_gate(
    gates: &mut BTreeMap<String, GateReceipt>,
    name: &str,
    result: Result<(), ComparatorError>,
) {
    let gate = match result {
        Ok(()) => GateReceipt {
            passed: true,
            detail: "passed".to_owned(),
        },
        Err(error) => GateReceipt {
            passed: false,
            detail: format!("{error:?}: {error}"),
        },
    };
    gates.insert(name.to_owned(), gate);
}

fn compare_semantic(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    let mut errors = Vec::new();
    for checkpoint in [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ] {
        let expected = read_snapshot(reference, checkpoint)?;
        let actual = read_snapshot(candidate, checkpoint)?;
        let masks = masks_for(scenario, checkpoint);
        if let Err(error) = super::cells::compare_cells(&expected, &actual, &masks) {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ComparatorError::Invalid {
            detail: errors.join("; "),
        })
    }
}

fn compare_pixels(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    let mut errors = Vec::new();
    for checkpoint in [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ] {
        let reference_png = artifact_path(reference, checkpoint, "terminal.png")?;
        let candidate_png = artifact_path(candidate, checkpoint, "terminal.png")?;
        let reference_bytes = read_file(&reference_png)?;
        let candidate_bytes = read_file(&candidate_png)?;
        let spans = pixel_spans(scenario, checkpoint, &reference_bytes)?;
        if let Err(error) =
            super::pixels::compare_png_bytes(&reference_bytes, &candidate_bytes, &spans)
        {
            errors.push(error.to_string());
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(ComparatorError::Invalid {
            detail: errors.join("; "),
        })
    }
}

fn compare_motion_gate(
    _scenario: &Scenario,
    capture: &DualRuntimeReceipt,
) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    super::motion::compare_checkpoint_motion(&reference.checkpoints, &candidate.checkpoints)
}

fn compare_timing_gate(capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    let reference_trace = timing_trace(reference)?;
    let candidate_trace = timing_trace(candidate)?;
    super::timing::compare_timing(&reference_trace, &candidate_trace)
}

fn compare_provenance(capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    super::self_compare::reject_self_comparison(
        &reference.binary.sha256,
        &candidate.binary.sha256,
    )?;
    if reference.binary.path == candidate.binary.path {
        return Err(ComparatorError::SelfComparison {
            sha256: reference.binary.sha256.clone(),
        });
    }
    for runtime in [reference, candidate] {
        if runtime.binary.source_revision.is_empty() || runtime.binary.sha256.len() != 64 {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "{} binary provenance is incomplete",
                    runtime.adapter.as_str()
                ),
            });
        }
        for checkpoint in &runtime.checkpoints {
            for artifact in &checkpoint.artifacts {
                let observed = sha256_file(Path::new(&artifact.path))?;
                if observed != artifact.sha256 {
                    return Err(ComparatorError::Hashing {
                        stale: vec![super::hashing::StaleArtifact {
                            kind: artifact.path.clone(),
                            expected: artifact.sha256.clone(),
                            observed,
                        }],
                        stale_len: 1,
                    });
                }
            }
        }
    }
    Ok(())
}

fn compare_checkpoints(capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let required: BTreeSet<CheckpointName> = [
        CheckpointName::Rest,
        CheckpointName::Mid,
        CheckpointName::Settled,
    ]
    .into_iter()
    .collect();
    for runtime in &capture.runtimes {
        let names: BTreeSet<CheckpointName> =
            runtime.checkpoints.iter().map(|item| item.name).collect();
        if names != required || runtime.checkpoints.len() != required.len() {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "{} checkpoint set is missing, duplicated, or unexpected",
                    runtime.adapter.as_str()
                ),
            });
        }
        for checkpoint in &runtime.checkpoints {
            for name in REQUIRED_ARTIFACTS {
                if !checkpoint
                    .artifacts
                    .iter()
                    .any(|artifact| artifact.path.ends_with(name))
                {
                    return Err(ComparatorError::Invalid {
                        detail: format!(
                            "{} {} missing {name}",
                            runtime.adapter.as_str(),
                            checkpoint.name.as_str()
                        ),
                    });
                }
            }
        }
    }
    Ok(())
}

fn compare_exits(scenario: &Scenario, capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    for runtime in &capture.runtimes {
        if runtime.normal_exit_code != scenario.expected_exit.code {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "{} exit {} != {}",
                    runtime.adapter.as_str(),
                    runtime.normal_exit_code,
                    scenario.expected_exit.code
                ),
            });
        }
    }
    Ok(())
}

fn compare_cleanup(cleanup: &CleanupReceipt) -> Result<(), ComparatorError> {
    if cleanup.status != "clean"
        || cleanup.forced_termination_observed
        || !cleanup.detected_child_pids.is_empty()
        || !cleanup.surviving_pids.is_empty()
        || !cleanup.cleanup_errors.is_empty()
    {
        return Err(ComparatorError::Invalid {
            detail: "cleanup receipt is not clean".to_owned(),
        });
    }
    Ok(())
}

fn runtime_pair(
    capture: &DualRuntimeReceipt,
) -> Result<(&AdapterReceipt, &AdapterReceipt), ComparatorError> {
    if capture.runtimes.len() != 2 {
        return Err(ComparatorError::Invalid {
            detail: "capture must contain exactly reference and candidate runtimes".to_owned(),
        });
    }
    let reference = capture
        .runtimes
        .iter()
        .find(|runtime| runtime.adapter.as_str() == "grok")
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "reference runtime is missing".to_owned(),
        })?;
    let candidate = capture
        .runtimes
        .iter()
        .find(|runtime| runtime.adapter.as_str() == "harness")
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "candidate runtime is missing".to_owned(),
        })?;
    Ok((reference, candidate))
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

fn artifact_path(
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

fn masks_for(scenario: &Scenario, checkpoint: CheckpointName) -> IdentityMaskRegistry {
    let mut masks = IdentityMaskRegistry::new();
    for substitution in scenario
        .substitutions
        .iter()
        .filter(|item| item.checkpoint == checkpoint)
    {
        let cells = (substitution.rectangle.row
            ..substitution
                .rectangle
                .row
                .saturating_add(substitution.rectangle.rows))
            .flat_map(|row| {
                (substitution.rectangle.col
                    ..substitution
                        .rectangle
                        .col
                        .saturating_add(substitution.rectangle.cols))
                    .map(move |col| (row, col))
            })
            .collect::<Vec<_>>();
        masks = masks.with_field(substitution.scope.placeholder(), cells);
    }
    masks
}

fn pixel_spans(
    scenario: &Scenario,
    checkpoint: CheckpointName,
    png: &[u8],
) -> Result<Vec<super::pixels::IdentityPixelSpan>, ComparatorError> {
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
    if viewport.cols == 0 || viewport.rows == 0 {
        return Err(ComparatorError::Invalid {
            detail: "scenario checkpoint viewport is empty".to_owned(),
        });
    }
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
        .map(|item: &IdentitySubstitution| {
            super::pixels::IdentityPixelSpan::from_cell_rect(
                item.scope.placeholder(),
                item.rectangle,
                cell_width,
                cell_height,
            )
        })
        .collect())
}

fn timing_trace(runtime: &AdapterReceipt) -> Result<super::timing::TimingTrace, ComparatorError> {
    let phase_order = runtime
        .checkpoints
        .iter()
        .map(|checkpoint| match checkpoint.name {
            CheckpointName::Rest => super::timing::TimingPhase::Rest,
            CheckpointName::Mid => super::timing::TimingPhase::Mid,
            CheckpointName::Settled => super::timing::TimingPhase::Settled,
        })
        .collect();
    Ok(super::timing::TimingTrace::with_phase_order(
        checkpoint_times(runtime)?,
        relative_input_times(runtime)?,
        phase_order,
    ))
}

fn relative_input_times(runtime: &AdapterReceipt) -> Result<Vec<u64>, ComparatorError> {
    let timestamps = runtime
        .input_timestamps_millis
        .iter()
        .map(|value| {
            u64::try_from(*value).map_err(|_| ComparatorError::Invalid {
                detail: "input timestamp exceeds u64".to_owned(),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let start = timestamps.first().copied().unwrap_or_default();
    Ok(timestamps
        .into_iter()
        .map(|timestamp| timestamp.saturating_sub(start))
        .collect())
}

fn checkpoint_times(runtime: &AdapterReceipt) -> Result<Vec<u64>, ComparatorError> {
    runtime
        .checkpoints
        .iter()
        .map(|checkpoint| {
            u64::try_from(checkpoint.captured_at_millis).map_err(|_| ComparatorError::Invalid {
                detail: "checkpoint timestamp exceeds u64".to_owned(),
            })
        })
        .collect()
}

fn read_file(path: &Path) -> Result<Vec<u8>, ComparatorError> {
    std::fs::read(path).map_err(|error| ComparatorError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })
}

fn sha256_file(path: &Path) -> Result<String, ComparatorError> {
    let mut command = Command::new("sha256sum");
    command
        .arg("--")
        .arg(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().map_err(|error| ComparatorError::Io {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    if !output.status.success() {
        return Err(ComparatorError::Io {
            path: path.to_path_buf(),
            detail: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: format!("sha256sum returned no digest for {}", path.display()),
        })
}
