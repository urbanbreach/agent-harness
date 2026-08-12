use std::collections::{HashMap, HashSet};

use super::{monotonic, require_nonempty, PresentationValidationError};
use crate::tui_fidelity_runner::presentation_receipt::{
    NativeAckOutcome, NativeCauseOutcome, NativePresentationTrace,
};

pub(super) fn validate(trace: &NativePresentationTrace) -> Result<(), PresentationValidationError> {
    require_nonempty("native.causes", &trace.causes)?;
    require_nonempty("native.demands", &trace.demands)?;
    require_nonempty("native.frames", &trace.frames)?;
    require_nonempty("native.acknowledgements", &trace.acknowledgements)?;
    monotonic(
        "native.causes",
        trace.causes.iter().map(|cause| cause.received_at.0),
    )?;
    if trace
        .frames
        .windows(2)
        .any(|pair| pair[1].sequence <= pair[0].sequence)
    {
        return Err(PresentationValidationError::FrameSequenceOrder);
    }
    if trace.acknowledgements.len() != trace.frames.len() {
        let sequence = trace
            .acknowledgements
            .iter()
            .find(|ack| {
                trace
                    .acknowledgements
                    .iter()
                    .filter(|other| other.sequence == ack.sequence)
                    .count()
                    != 1
                    || !trace
                        .frames
                        .iter()
                        .any(|frame| frame.sequence == ack.sequence)
            })
            .map_or(0, |ack| ack.sequence);
        return Err(PresentationValidationError::AckCardinality { sequence });
    }
    let cause_ids = trace
        .causes
        .iter()
        .map(|cause| cause.cause_id.as_str())
        .collect::<HashSet<_>>();
    let acknowledgements = trace.acknowledgements.iter().fold(
        HashMap::<u64, Vec<_>>::new(),
        |mut grouped, acknowledgement| {
            grouped
                .entry(acknowledgement.sequence)
                .or_default()
                .push(acknowledgement);
            grouped
        },
    );
    for frame in &trace.frames {
        if frame
            .cause_ids
            .iter()
            .any(|id| !cause_ids.contains(id.as_str()))
        {
            return Err(PresentationValidationError::UnresolvedReference {
                detail: format!("frame {} cause", frame.sequence),
            });
        }
        let Some(frame_acks) = acknowledgements.get(&frame.sequence) else {
            return Err(PresentationValidationError::AckCardinality {
                sequence: frame.sequence,
            });
        };
        if frame_acks.len() != 1 {
            return Err(PresentationValidationError::AckCardinality {
                sequence: frame.sequence,
            });
        }
        let acknowledgement = frame_acks[0];
        if acknowledgement.outcome != NativeAckOutcome::CompletedWrite
            || acknowledgement.revision != frame.revision
            || acknowledgement.byte_sha256 != frame.byte_sha256
            || acknowledgement.write_ended_at > acknowledgement.acknowledged_at
        {
            return Err(PresentationValidationError::AckMismatch {
                sequence: frame.sequence,
            });
        }
    }
    validate_visible_causes(trace, &acknowledgements)
}

fn validate_visible_causes(
    trace: &NativePresentationTrace,
    acknowledgements: &HashMap<u64, Vec<&crate::tui_fidelity_runner::NativeFrameAck>>,
) -> Result<(), PresentationValidationError> {
    for cause in trace
        .causes
        .iter()
        .filter(|cause| cause.outcome == NativeCauseOutcome::VisibleChange)
    {
        let presented = trace.frames.iter().any(|frame| {
            frame.cause_ids.contains(&cause.cause_id)
                && cause
                    .resulting_revision
                    .is_some_and(|revision| frame.revision >= revision)
                && acknowledgements[&frame.sequence][0].outcome == NativeAckOutcome::CompletedWrite
        });
        if !presented {
            return Err(PresentationValidationError::VisibleCauseUnpresented {
                cause_id: cause.cause_id.clone(),
            });
        }
    }
    Ok(())
}
