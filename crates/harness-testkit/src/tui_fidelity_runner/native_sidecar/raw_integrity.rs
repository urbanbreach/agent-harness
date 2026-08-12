use super::{invalid, RuntimeAck, RuntimeFrame, RuntimeTrace, RuntimeTraceOutcome};
use crate::tui_fidelity_runner::RunnerError;

pub(super) fn validate(trace: &RuntimeTrace) -> Result<(), RunnerError> {
    if trace.frames.len() != trace.acknowledgements.len() {
        return Err(invalid("native frame/ack count differs"));
    }
    for (frame, acknowledgement) in trace.frames.iter().zip(&trace.acknowledgements) {
        validate_pair(frame, acknowledgement)?;
    }
    for outcome in &trace.outcomes {
        if let RuntimeTraceOutcome::ResyncRequired {
            rejected_revision,
            replacement_revision,
            recorded_at,
        } = outcome
        {
            let linked = replacement_revision > rejected_revision
                && *recorded_at > 0
                && trace
                    .demands
                    .iter()
                    .any(|demand| demand.target_revision == *replacement_revision)
                && trace
                    .frames
                    .iter()
                    .any(|frame| frame.revision == *replacement_revision);
            if !linked {
                return Err(invalid(
                    "native resync outcome is not linked to replacement",
                ));
            }
        }
    }
    Ok(())
}

fn validate_pair(frame: &RuntimeFrame, ack: &RuntimeAck) -> Result<(), RunnerError> {
    let outcome_agrees = frame.acknowledgement == ack.outcome;
    let agrees = frame.sequence == ack.sequence
        && frame.revision == ack.revision
        && frame.cause_ids == ack.cause_ids
        && frame.requested_at == ack.requested_at
        && frame.render_started_at == ack.render_started_at
        && frame.render_ended_at == ack.render_ended_at
        && frame.submitted_at == ack.submitted_at
        && frame.write_started_at == ack.write_started_at
        && frame.write_ended_at == ack.write_ended_at
        && frame.acknowledged_at == ack.acknowledged_at
        && frame.frame_kind == ack.frame_kind
        && frame.byte_count == ack.byte_count
        && frame.byte_sha256 == ack.byte_sha256
        && outcome_agrees;
    if agrees {
        Ok(())
    } else {
        Err(invalid(&format!(
            "native frame/ack integrity differs at sequence {}",
            frame.sequence
        )))
    }
}
