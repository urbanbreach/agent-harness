use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

use crate::tui_fidelity_runner::{
    NativeAckOutcome, NativeCauseOutcome, PresentationEvidence, PresentationTimestamp,
};

use super::error::ComparatorError;
use super::types::PresentationComparisonMetrics;

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
    derive_presentation_timing_excluding(evidence, &[])
}

pub(crate) fn derive_presentation_timing_excluding(
    evidence: &PresentationEvidence,
    excluded_action_ordinals: &[usize],
) -> Result<PresentationTimingMetrics, ComparatorError> {
    let (external, native) = match evidence {
        PresentationEvidence::ExternalOnly { external } => (external, None),
        PresentationEvidence::HarnessNative {
            external, native, ..
        } => (external, Some(native)),
    };
    let mut samples = Vec::with_capacity(external.actual_input_sends.len());
    for send in external
        .actual_input_sends
        .iter()
        .filter(|send| !excluded_action_ordinals.contains(&send.action_ordinal))
    {
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
    let scenario_epoch = scenario_epoch(external)?;
    let timestamps = external
        .observations
        .iter()
        .filter(|observation| {
            observation.kind != crate::tui_fidelity_runner::ObservationKind::StableRepeat
                && observation.observed_at >= scenario_epoch
        })
        .map(|observation| observation.observed_at.0)
        .collect::<Vec<_>>();
    let observation_intervals =
        external_epoch_intervals(external, excluded_action_ordinals, scenario_epoch);
    Ok(PresentationTimingMetrics {
        external_send_to_changed_observation_micros: samples,
        external_observation_timestamps_micros: timestamps,
        external_cadence_micros: median_nonzero(&observation_intervals),
        external_observation_intervals_micros: observation_intervals,
        native: native
            .map(|trace| derive_native(trace, external, excluded_action_ordinals))
            .transpose()?,
    })
}

pub fn derive_comparison_presentation_timing(
    reference: &PresentationEvidence,
    candidate: &PresentationEvidence,
) -> Result<PresentationComparisonMetrics, ComparatorError> {
    let excluded = no_visible_action_ordinals(candidate);
    Ok(PresentationComparisonMetrics {
        reference: derive_presentation_timing_excluding(reference, &excluded)?,
        candidate: derive_presentation_timing_excluding(candidate, &excluded)?,
    })
}

fn no_visible_action_ordinals(presentation: &PresentationEvidence) -> Vec<usize> {
    let PresentationEvidence::HarnessNative {
        external, native, ..
    } = presentation
    else {
        return Vec::new();
    };
    external
        .actual_input_sends
        .iter()
        .filter(|send| {
            let causes = native
                .causes
                .iter()
                .filter(|cause| cause.interaction_id.as_ref() == Some(&send.interaction_id))
                .collect::<Vec<_>>();
            !causes.is_empty()
                && causes.iter().all(|cause| {
                    cause.outcome == NativeCauseOutcome::NoVisibleChange
                        && cause.resulting_revision.is_none()
                })
        })
        .map(|send| send.action_ordinal)
        .collect()
}

fn derive_native(
    trace: &crate::tui_fidelity_runner::NativePresentationTrace,
    external: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
    excluded_action_ordinals: &[usize],
) -> Result<NativeTimingMetrics, ComparatorError> {
    let successful = trace
        .acknowledgements
        .iter()
        .filter(|ack| ack.outcome == NativeAckOutcome::CompletedWrite)
        .collect::<Vec<_>>();
    let receive = native_input_latencies(trace, external, excluded_action_ordinals, &successful)?;
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
    let completed_intervals = native_epoch_intervals(trace, &successful);
    Ok(NativeTimingMetrics {
        receive_to_successful_flush_micros: receive,
        request_to_successful_flush_micros: request,
        completed_write_intervals_micros: completed_intervals,
        completed_write_timestamps_micros: completed,
        coalesced_requests: trace.aggregates.coalesced_requests,
        queue_saturation: trace.aggregates.queue_saturation,
        resyncs: trace.aggregates.resyncs,
        full_repaints: trace.aggregates.full_repaints,
        bytes_written: trace.aggregates.bytes_written,
        idle_redraws: trace.aggregates.idle_redraws,
    })
}

fn external_epoch_intervals(
    external: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
    excluded_action_ordinals: &[usize],
    scenario_epoch: PresentationTimestamp,
) -> Vec<u64> {
    let observations = external
        .observations
        .iter()
        .filter(|observation| {
            observation.kind != crate::tui_fidelity_runner::ObservationKind::StableRepeat
                && observation.observed_at >= scenario_epoch
        })
        .collect::<Vec<_>>();
    observations
        .windows(2)
        .filter(|window| {
            !external.actual_input_sends.iter().any(|send| {
                !excluded_action_ordinals.contains(&send.action_ordinal)
                    && send.sent_at > window[0].observed_at
                    && send.sent_at <= window[1].observed_at
            })
        })
        .map(|window| {
            window[1]
                .observed_at
                .0
                .saturating_sub(window[0].observed_at.0)
        })
        .collect()
}

fn scenario_epoch(
    external: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
) -> Result<PresentationTimestamp, ComparatorError> {
    let first = external
        .action_receipts
        .first()
        .ok_or_else(|| ComparatorError::Invalid {
            detail: "external timing requires action receipts".to_owned(),
        })?;
    let mut interaction_ids = HashSet::new();
    let mut previous_scheduled_at = None;
    for (ordinal, receipt) in external.action_receipts.iter().enumerate() {
        if receipt.action_ordinal != ordinal
            || !interaction_ids.insert(&receipt.interaction_id)
            || receipt.scheduled_at > receipt.started_at
            || receipt.started_at > receipt.ended_at
            || previous_scheduled_at.is_some_and(|previous| previous > receipt.scheduled_at)
        {
            return Err(ComparatorError::Invalid {
                detail: format!(
                    "external timing action receipt {} is malformed",
                    receipt.interaction_id.0
                ),
            });
        }
        previous_scheduled_at = Some(receipt.scheduled_at);
    }
    Ok(first.scheduled_at)
}

fn native_input_latencies(
    trace: &crate::tui_fidelity_runner::NativePresentationTrace,
    external: &crate::tui_fidelity_runner::ExternalPresentationEvidence,
    excluded_action_ordinals: &[usize],
    successful: &[&crate::tui_fidelity_runner::NativeFrameAck],
) -> Result<Vec<u64>, ComparatorError> {
    let mut samples = Vec::new();
    for send in external
        .actual_input_sends
        .iter()
        .filter(|send| !excluded_action_ordinals.contains(&send.action_ordinal))
    {
        let causes = trace
            .causes
            .iter()
            .filter(|cause| cause.interaction_id.as_ref() == Some(&send.interaction_id))
            .filter(|cause| cause.outcome == NativeCauseOutcome::VisibleChange)
            .collect::<Vec<_>>();
        if causes.is_empty() {
            continue;
        }
        let received_at = causes
            .iter()
            .map(|cause| cause.received_at)
            .max()
            .ok_or_else(|| ComparatorError::Invalid {
                detail: format!(
                    "interaction {} has no native receive",
                    send.interaction_id.0
                ),
            })?;
        let final_cause_ids = causes
            .iter()
            .filter(|cause| cause.received_at == received_at)
            .map(|cause| cause.cause_id.as_str())
            .collect::<Vec<_>>();
        let ack = successful
            .iter()
            .filter(|ack| {
                ack.write_ended_at >= received_at
                    && trace.frames.iter().any(|frame| {
                        frame.sequence == ack.sequence
                            && final_cause_ids.iter().any(|cause_id| {
                                frame.cause_ids.iter().any(|frame_id| frame_id == cause_id)
                            })
                    })
            })
            .min_by_key(|ack| ack.write_ended_at)
            .ok_or_else(|| ComparatorError::Invalid {
                detail: format!(
                    "interaction {} has no successful causal flush",
                    send.interaction_id.0
                ),
            })?;
        samples.push(delta(
            ack.write_ended_at,
            received_at,
            "native input receive-to-flush",
        )?);
    }
    Ok(samples)
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum NativeEpoch {
    Animation,
    LiveUpdate,
    Interaction(String),
    Other(String),
}

fn native_epoch_intervals(
    trace: &crate::tui_fidelity_runner::NativePresentationTrace,
    successful: &[&crate::tui_fidelity_runner::NativeFrameAck],
) -> Vec<u64> {
    let causes = trace
        .causes
        .iter()
        .map(|cause| (cause.cause_id.as_str(), cause))
        .collect::<BTreeMap<_, _>>();
    let samples = successful
        .iter()
        .filter_map(|ack| {
            let frame = trace
                .frames
                .iter()
                .find(|frame| frame.sequence == ack.sequence)?;
            let frame_causes = frame
                .cause_ids
                .iter()
                .filter_map(|cause_id| causes.get(cause_id.as_str()).copied())
                .collect::<Vec<_>>();
            let epoch = frame_causes
                .iter()
                .find_map(|cause| cause.interaction_id.as_ref())
                .map(|interaction_id| NativeEpoch::Interaction(interaction_id.0.clone()))
                .or_else(|| {
                    (!frame_causes.is_empty()
                        && frame_causes
                            .iter()
                            .all(|cause| cause.kind == "animation_timer"))
                    .then_some(NativeEpoch::Animation)
                })
                .or_else(|| {
                    (!frame_causes.is_empty()
                        && frame_causes.iter().all(|cause| cause.kind == "live_update"))
                    .then_some(NativeEpoch::LiveUpdate)
                })
                .unwrap_or_else(|| {
                    NativeEpoch::Other(
                        frame_causes
                            .first()
                            .map_or_else(|| "unattributed".to_owned(), |cause| cause.kind.clone()),
                    )
                });
            Some((epoch, ack.write_ended_at.0))
        })
        .collect::<Vec<_>>();
    let mut samples = samples;
    samples.sort_by_key(|sample| sample.1);
    samples
        .windows(2)
        .filter(|window| window[0].0 == window[1].0)
        .map(|window| window[1].1.saturating_sub(window[0].1))
        .collect()
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
