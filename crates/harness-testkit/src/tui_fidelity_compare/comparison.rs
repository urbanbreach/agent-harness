mod presentation;
mod provenance;
mod semantic_pixels;

use std::collections::{BTreeMap, BTreeSet};

use crate::tui_fidelity::{CheckpointName, Scenario};
use crate::tui_fidelity_runner::{AdapterReceipt, CleanupReceipt, DualRuntimeReceipt};

use super::error::ComparatorError;
use super::types::{AcceptanceProfile, ComparisonReceipt, GateReceipt, COMPARISON_RECEIPT_SCHEMA};

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
    compare_capture_with_profile(scenario, capture, cleanup, AcceptanceProfile::FullParity)
}

pub fn compare_capture_with_profile(
    scenario: &Scenario,
    capture: &DualRuntimeReceipt,
    cleanup: &CleanupReceipt,
    profile: AcceptanceProfile,
) -> ComparisonReceipt {
    let mut gates = BTreeMap::new();
    record_gate(
        &mut gates,
        "presentation",
        presentation::validate(scenario, capture),
    );
    record_gate(
        &mut gates,
        "semantic_cell",
        semantic_pixels::semantic(scenario, capture),
    );
    record_gate(
        &mut gates,
        "pixel",
        semantic_pixels::pixels(scenario, capture),
    );
    record_gate(
        &mut gates,
        "motion",
        presentation::motion(scenario, capture),
    );
    let presentation = presentation::metrics(capture).ok();
    record_gate(&mut gates, "timing", presentation::timing(capture));
    record_gate(&mut gates, "provenance", provenance::compare(capture));
    record_gate(&mut gates, "checkpoint", compare_checkpoints(capture));
    record_gate(&mut gates, "exit", compare_exits(scenario, capture));
    record_gate(&mut gates, "cleanup", compare_cleanup(cleanup));
    let capture_succeeded = gates.get("presentation").is_some_and(|gate| gate.passed);
    let comparison_passed = capture_succeeded
        && match profile {
            AcceptanceProfile::FullParity => gates.values().all(|gate| gate.passed),
            AcceptanceProfile::Packet2Scheduling => [
                "presentation",
                "provenance",
                "checkpoint",
                "exit",
                "cleanup",
            ]
            .iter()
            .all(|name| gates.get(*name).is_some_and(|gate| gate.passed)),
        };
    ComparisonReceipt {
        schema_version: COMPARISON_RECEIPT_SCHEMA.to_owned(),
        acceptance_profile: profile,
        capture_succeeded,
        comparison_passed,
        gates,
        presentation,
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
