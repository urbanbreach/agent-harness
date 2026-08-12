use std::collections::HashSet;

use crate::parity::{MotionFamily, MotionPhase};

use super::error::{MotionCaptureError, ScenarioError};
use super::types::{CheckpointName, MotionBoundary, MotionObservationRule, Scenario};

pub(super) fn validate(scenario: &Scenario) -> Result<(), ScenarioError> {
    let contract = &scenario.motion_capture;
    if contract.families.is_empty() {
        return invalid(MotionCaptureError::NoFamilies);
    }
    if contract.markers.is_empty() {
        return invalid(MotionCaptureError::NoMarkers);
    }
    let mut families = HashSet::new();
    if contract
        .families
        .iter()
        .any(|family| !families.insert(*family))
    {
        return invalid(MotionCaptureError::DuplicateFamily);
    }
    let mut previous_rank = None;
    for (index, marker) in contract.markers.iter().enumerate() {
        let rank = boundary_rank(marker.boundary, scenario.actions.len(), index)?;
        if previous_rank.is_some_and(|previous| rank <= previous) {
            return invalid(MotionCaptureError::MarkerOutOfOrder { marker: index });
        }
        previous_rank = Some(rank);
        if !phase_is_selected(marker.phase, &families) {
            return invalid(MotionCaptureError::IncompatiblePhase { marker: index });
        }
        let expected = if marker.observation == MotionObservationRule::StableRepeat {
            3
        } else {
            1
        };
        if marker.repeat_count != expected {
            return invalid(MotionCaptureError::InvalidRepeatCount { marker: index });
        }
    }
    if !matches!(contract.markers.last(), Some(marker)
        if marker.phase == MotionPhase::SettleRepeat
        && marker.boundary == (MotionBoundary::Checkpoint { name: CheckpointName::Settled })
        && marker.observation == MotionObservationRule::StableRepeat
        && marker.repeat_count == 3)
    {
        return invalid(MotionCaptureError::MissingTerminalSettle);
    }
    Ok(())
}

fn boundary_rank(
    boundary: MotionBoundary,
    count: usize,
    marker: usize,
) -> Result<usize, ScenarioError> {
    match boundary {
        MotionBoundary::BeforeAction { ordinal } | MotionBoundary::AfterAction { ordinal }
            if ordinal >= count =>
        {
            invalid(MotionCaptureError::BoundaryOutOfRange { marker, ordinal })
        }
        MotionBoundary::BeforeAction { ordinal } => Ok(ordinal * 2),
        MotionBoundary::AfterAction { ordinal } => Ok(ordinal * 2 + 1),
        MotionBoundary::Checkpoint { name } => Ok(count * 2 + checkpoint_rank(name)),
    }
}

const fn checkpoint_rank(name: CheckpointName) -> usize {
    match name {
        CheckpointName::Rest => 0,
        CheckpointName::Mid => 1,
        CheckpointName::Settled => 2,
    }
}

fn phase_is_selected(phase: MotionPhase, families: &HashSet<MotionFamily>) -> bool {
    let family = match phase {
        MotionPhase::Startup | MotionPhase::FinishFlash => MotionFamily::OrderedMotion,
        MotionPhase::StreamingDelta => MotionFamily::StreamingDeltas,
        MotionPhase::ScrollFlush => MotionFamily::ScrollFlush,
        MotionPhase::ResizeBurst | MotionPhase::ResizeSettled => MotionFamily::ResizeDebounce,
        MotionPhase::Cancellation | MotionPhase::CancelRecovered => {
            MotionFamily::CancellationOrdering
        }
        MotionPhase::SettleRepeat => MotionFamily::SettleDwell,
    };
    families.contains(&family)
}

fn invalid<T>(error: MotionCaptureError) -> Result<T, ScenarioError> {
    Err(ScenarioError::InvalidMotionCapture(error))
}
