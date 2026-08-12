use crate::parity::{
    compare_ordered_motion_traces, FrameTrace, IdentityMaskRegistry, TickFrame, TraceSource,
};
use crate::tui_fidelity::{MotionBoundary, MotionObservationRule, Scenario};
use crate::tui_fidelity_runner::{AdapterReceipt, ObservationKind, PresentationEvidence};

use super::error::ComparatorError;
use super::motion::MotionIssue;

pub fn normalize_ordered_motion(
    scenario: &Scenario,
    presentation: &PresentationEvidence,
    source: TraceSource,
) -> Result<FrameTrace, ComparatorError> {
    let external = match presentation {
        PresentationEvidence::ExternalOnly { external }
        | PresentationEvidence::HarnessNative { external, .. } => external,
    };
    let mut frames = Vec::new();
    let mut cursor = 0;
    for marker in &scenario.motion_capture.markers {
        let selected = match marker.observation {
            MotionObservationRule::StableRepeat => external
                .observations
                .iter()
                .enumerate()
                .filter(|(_, item)| item.kind == ObservationKind::StableRepeat)
                .rev()
                .take(usize::from(marker.repeat_count))
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect::<Vec<_>>(),
            MotionObservationRule::LastChangedBeforeStable => external
                .observations
                .iter()
                .enumerate()
                .filter(|(_, item)| item.kind != ObservationKind::StableRepeat)
                .next_back()
                .into_iter()
                .collect(),
            MotionObservationRule::FirstChanged => match marker.boundary {
                MotionBoundary::BeforeAction { ordinal } => {
                    let boundary = action_time(ordinal, external).unwrap_or(u64::MAX);
                    external
                        .observations
                        .iter()
                        .enumerate()
                        .filter(|(_, item)| {
                            item.kind != ObservationKind::StableRepeat
                                && item.observed_at.0 <= boundary
                        })
                        .next_back()
                        .into_iter()
                        .collect()
                }
                MotionBoundary::AfterAction { ordinal } => {
                    let boundary = action_time(ordinal, external).unwrap_or_default();
                    external
                        .observations
                        .iter()
                        .enumerate()
                        .skip(cursor)
                        .find(|(_, item)| {
                            item.kind != ObservationKind::StableRepeat
                                && item.observed_at.0 >= boundary
                        })
                        .into_iter()
                        .collect()
                }
                MotionBoundary::Checkpoint { .. } => external
                    .observations
                    .iter()
                    .enumerate()
                    .skip(cursor)
                    .find(|(_, item)| item.kind != ObservationKind::StableRepeat)
                    .into_iter()
                    .collect(),
            },
            MotionObservationRule::EachChanged => {
                let minimum = boundary_time(marker.boundary, external).unwrap_or_default();
                external
                    .observations
                    .iter()
                    .enumerate()
                    .skip(cursor)
                    .filter(|(_, item)| {
                        item.kind != ObservationKind::StableRepeat && item.observed_at.0 >= minimum
                    })
                    .take(usize::from(marker.repeat_count))
                    .collect()
            }
        };
        if selected.len() != usize::from(marker.repeat_count) {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "motion marker {} requires {} observations, found {}",
                    marker.phase,
                    marker.repeat_count,
                    selected.len()
                ),
            });
        }
        for (ordinal, observation) in selected {
            frames.push(TickFrame {
                tick: observation.observed_at.0,
                phase: marker.phase,
                frame: observation.frame.clone(),
            });
            cursor = cursor.max(ordinal.saturating_add(1));
        }
    }
    Ok(FrameTrace { source, frames })
}

pub fn compare_ordered_motion(
    scenario: &Scenario,
    reference: &AdapterReceipt,
    candidate: &AdapterReceipt,
) -> Result<(), ComparatorError> {
    compare_ordered_presentations(scenario, &reference.presentation, &candidate.presentation)
}

pub fn compare_ordered_presentations(
    scenario: &Scenario,
    reference: &PresentationEvidence,
    candidate: &PresentationEvidence,
) -> Result<(), ComparatorError> {
    if scenario.motion_capture.families.is_empty() {
        return Err(ComparatorError::Invalid {
            detail: "scenario declares no motion families".to_owned(),
        });
    }
    let reference = normalize_ordered_motion(scenario, reference, TraceSource::Reference)?;
    let candidate = normalize_ordered_motion(scenario, candidate, TraceSource::Harness)?;
    let masks = masks(scenario);
    compare_ordered_motion_traces(
        &reference,
        &candidate,
        &masks,
        &scenario.motion_capture.families,
    )
    .map_err(|defects| {
        let defects = defects
            .into_iter()
            .map(|defect| MotionIssue {
                side: "ordered_pty".to_owned(),
                reason: defect.reason,
                detail: defect.detail,
            })
            .collect::<Vec<_>>();
        ComparatorError::Motion {
            defects_len: defects.len(),
            defects,
        }
    })
}

fn action_time(
    ordinal: usize,
    evidence: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
) -> Option<u64> {
    evidence
        .actual_input_sends
        .iter()
        .find(|send| send.action_ordinal == ordinal)
        .map(|send| send.sent_at.0)
}

fn boundary_time(
    boundary: MotionBoundary,
    evidence: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
) -> Option<u64> {
    match boundary {
        MotionBoundary::BeforeAction { ordinal } | MotionBoundary::AfterAction { ordinal } => {
            action_time(ordinal, evidence)
        }
        MotionBoundary::Checkpoint { .. } => None,
    }
}

fn masks(scenario: &Scenario) -> IdentityMaskRegistry {
    scenario
        .substitutions
        .iter()
        .fold(IdentityMaskRegistry::new(), |masks, substitution| {
            let cells: Vec<(u16, u16)> = (substitution.rectangle.row
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
                .collect();
            masks.with_field(substitution.scope.placeholder(), cells)
        })
}
