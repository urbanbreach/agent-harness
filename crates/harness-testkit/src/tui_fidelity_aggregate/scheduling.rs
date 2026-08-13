use serde::Deserialize;

use super::helpers::{evidence, read_json, verify_artifact};
use super::types::Artifact;
use super::AggregateError;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulingSidecar {
    schema_version: String,
    actual_input_sends: Vec<SchedulingInputSend>,
    maximum_backlog_depth: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SchedulingInputSend {
    interaction_id: String,
    action_ordinal: usize,
    terminal_sequence: u64,
    source_decision: String,
    live_ready_depth: u64,
    queued_live_depth: u64,
    deferred_live_ready: bool,
    stream_active: bool,
    preempted_live: bool,
    fairness_yield: bool,
    deadline_millis: Option<u64>,
    cause_id: String,
}

pub(super) fn verify(
    artifact: &Artifact,
    expected_order: &[(usize, String)],
) -> Result<(), AggregateError> {
    verify_artifact(artifact)?;
    let sidecar: SchedulingSidecar = read_json(&artifact.path)?;
    let observed = sidecar
        .actual_input_sends
        .iter()
        .map(|send| (send.action_ordinal, send.interaction_id.as_str()))
        .collect::<Vec<_>>();
    if sidecar.schema_version != "harness.packet2-scheduling.v1" {
        return evidence(&artifact.path, "invalid scheduling sidecar schema");
    }
    let expected = expected_order
        .iter()
        .map(|(ordinal, interaction)| (*ordinal, interaction.as_str()))
        .collect::<Vec<_>>();
    if observed != expected {
        let first = observed
            .iter()
            .zip(&expected)
            .position(|(left, right)| left != right)
            .unwrap_or(observed.len().min(expected_order.len()));
        return evidence(
            &artifact.path,
            &format!("scheduling input order differs at interaction {first}"),
        );
    }
    let persisted_maximum = sidecar
        .actual_input_sends
        .iter()
        .map(|send| send.live_ready_depth)
        .max()
        .unwrap_or_default();
    if sidecar.maximum_backlog_depth != persisted_maximum {
        return evidence(
            &artifact.path,
            "scheduling maximum backlog differs from persisted action sends",
        );
    }
    if persisted_maximum == 0 {
        return evidence(&artifact.path, "scheduling sidecar has no backlog proof");
    }
    if sidecar.actual_input_sends.iter().any(|send| {
        send.stream_active
            && send.queued_live_depth == 0
            && !send.deferred_live_ready
            && send.live_ready_depth > 0
    }) {
        return evidence(
            &artifact.path,
            "active stream was relabeled as queued live readiness",
        );
    }
    let decisions_valid = sidecar
        .actual_input_sends
        .iter()
        .enumerate()
        .all(|(index, send)| {
            let truthful_depth = send
                .queued_live_depth
                .saturating_add(u64::from(send.deferred_live_ready));
            send.terminal_sequence == u64::try_from(index).unwrap_or(u64::MAX) + 1
                && send.source_decision == "terminal_input"
                && send.live_ready_depth > 0
                && send.live_ready_depth == truthful_depth
                && send.preempted_live
                && send.preempted_live == (truthful_depth > 0)
                && send.deadline_millis.is_some()
                && !send.cause_id.is_empty()
        });
    if !decisions_valid {
        return evidence(
            &artifact.path,
            "scheduling sidecar lacks per-action backlog, preemption, deadline, or cause proof",
        );
    }
    Ok(())
}
