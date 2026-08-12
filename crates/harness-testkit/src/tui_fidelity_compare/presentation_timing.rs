use serde::{Deserialize, Serialize};

use crate::tui_fidelity_runner::{
    NativeAckOutcome, NativeCauseOutcome, PresentationEvidence, PresentationTimestamp,
};

use super::error::ComparatorError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeTimingMetrics {
    pub receive_to_successful_flush_micros: Vec<u64>,
    pub request_to_successful_flush_micros: Vec<u64>,
    pub completed_write_timestamps_micros: Vec<u64>,
    pub completed_write_intervals_micros: Vec<u64>,
    pub coalesced_requests: u64,
    pub queue_saturation: u64,
    pub resyncs: u64,
    pub full_repaints: u64,
    pub bytes_written: u64,
    pub idle_redraws: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationTimingMetrics {
    pub external_send_to_changed_observation_micros: Vec<u64>,
    pub external_observation_timestamps_micros: Vec<u64>,
    pub external_observation_intervals_micros: Vec<u64>,
    pub external_cadence_micros: u64,
    pub native: Option<NativeTimingMetrics>,
}

pub fn derive_presentation_timing(
    evidence: &PresentationEvidence,
) -> Result<PresentationTimingMetrics, ComparatorError> {
    let (external, native) = match evidence {
        PresentationEvidence::ExternalOnly { external } => (external, None),
        PresentationEvidence::HarnessNative {
            external, native, ..
        } => (external, Some(native)),
    };
    let mut samples = Vec::with_capacity(external.actual_input_sends.len());
    for send in &external.actual_input_sends {
        let observation = external
            .interaction_observations
            .iter()
            .find(|mapping| mapping.interaction_id == send.interaction_id)
            .and_then(|mapping| mapping.first_changed_observation)
            .and_then(|ordinal| external.observations.get(ordinal))
            .ok_or_else(|| ComparatorError::Invalid {
                detail: format!(
                    "interaction {} has no changed PTY observation",
                    send.interaction_id.0
                ),
            })?;
        samples.push(delta(
            observation.observed_at,
            send.sent_at,
            "external latency",
        )?);
    }
    let timestamps = external
        .observations
        .iter()
        .map(|observation| observation.observed_at.0)
        .collect::<Vec<_>>();
    let observation_intervals = intervals(&timestamps);
    Ok(PresentationTimingMetrics {
        external_send_to_changed_observation_micros: samples,
        external_observation_timestamps_micros: timestamps,
        external_cadence_micros: median_nonzero(&observation_intervals),
        external_observation_intervals_micros: observation_intervals,
        native: native.map(|trace| derive_native(trace)).transpose()?,
    })
}

fn derive_native(
    trace: &crate::tui_fidelity_runner::NativePresentationTrace,
) -> Result<NativeTimingMetrics, ComparatorError> {
    let successful = trace
        .acknowledgements
        .iter()
        .filter(|ack| ack.outcome == NativeAckOutcome::CompletedWrite)
        .collect::<Vec<_>>();
    let mut receive = Vec::new();
    for cause in trace
        .causes
        .iter()
        .filter(|cause| cause.outcome == NativeCauseOutcome::VisibleChange)
    {
        let revision = cause
            .resulting_revision
            .ok_or_else(|| ComparatorError::Invalid {
                detail: format!("visible cause {} has no revision", cause.cause_id),
            })?;
        let ack = successful
            .iter()
            .filter(|ack| ack.revision >= revision)
            .filter(|ack| {
                trace.frames.iter().any(|frame| {
                    frame.sequence == ack.sequence && frame.cause_ids.contains(&cause.cause_id)
                })
            })
            .min_by_key(|ack| ack.write_ended_at)
            .ok_or_else(|| ComparatorError::Invalid {
                detail: format!("visible cause {} has no successful flush", cause.cause_id),
            })?;
        receive.push(delta(
            ack.write_ended_at,
            cause.received_at,
            "native receive-to-flush",
        )?);
    }
    let completed = successful
        .iter()
        .map(|ack| ack.write_ended_at.0)
        .collect::<Vec<_>>();
    let request = successful
        .iter()
        .map(|ack| {
            let frame = trace
                .frames
                .iter()
                .find(|frame| frame.sequence == ack.sequence)
                .ok_or_else(|| ComparatorError::Invalid {
                    detail: format!("ack {} has no frame", ack.sequence),
                })?;
            delta(
                ack.write_ended_at,
                frame.requested_at,
                "native request-to-flush",
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(NativeTimingMetrics {
        receive_to_successful_flush_micros: receive,
        request_to_successful_flush_micros: request,
        completed_write_intervals_micros: intervals(&completed),
        completed_write_timestamps_micros: completed,
        coalesced_requests: trace.aggregates.coalesced_requests,
        queue_saturation: trace.aggregates.queue_saturation,
        resyncs: trace.aggregates.resyncs,
        full_repaints: trace.aggregates.full_repaints,
        bytes_written: trace.aggregates.bytes_written,
        idle_redraws: trace.aggregates.idle_redraws,
    })
}

fn delta(
    end: PresentationTimestamp,
    start: PresentationTimestamp,
    field: &str,
) -> Result<u64, ComparatorError> {
    end.0
        .checked_sub(start.0)
        .ok_or_else(|| ComparatorError::Invalid {
            detail: format!("{field} timestamp regressed"),
        })
}

pub(super) fn intervals(timestamps: &[u64]) -> Vec<u64> {
    timestamps
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0]))
        .collect()
}

pub(super) fn median_nonzero(values: &[u64]) -> u64 {
    let mut values = values
        .iter()
        .copied()
        .filter(|value| *value > 0)
        .collect::<Vec<_>>();
    values.sort_unstable();
    values.get(values.len() / 2).copied().unwrap_or_default()
}
