use std::path::Path;

use serde::Deserialize;

use super::presentation_receipt::{
    InteractionId, NativeAckOutcome, NativeCause, NativeCauseOutcome, NativeDemand, NativeFrame,
    NativeFrameAck, NativePresentationAggregates, NativePresentationTrace, PresentationTimestamp,
};
use super::RunnerError;

mod outcomes;
use outcomes::{convert_trace_outcome, RuntimeTraceOutcome};
mod raw_integrity;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeTrace {
    trace_id: String,
    causes: Vec<RuntimeCause>,
    demands: Vec<RuntimeDemand>,
    frames: Vec<RuntimeFrame>,
    acknowledgements: Vec<RuntimeAck>,
    outcomes: Vec<RuntimeTraceOutcome>,
    aggregates: NativePresentationAggregates,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeCause {
    cause_id: String,
    interaction_id: Option<String>,
    received_at: u64,
    kind: String,
    resulting_revision: Option<u64>,
    outcome: RuntimeCauseOutcome,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RuntimeCauseOutcome {
    Pending,
    VisibleChange {
        cause_id: String,
        revision: u64,
    },
    NoVisibleChange {
        cause_id: String,
        closed_at: u64,
    },
    ResyncRequired {
        rejected_revision: u64,
        replacement_revision: u64,
        recorded_at: u64,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeDemand {
    target_revision: u64,
    earliest_requested_at: u64,
    latest_requested_at: u64,
    cause_ids: Vec<String>,
    reason: String,
    coalesced_request_count: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeFrame {
    sequence: u64,
    revision: u64,
    cause_ids: Vec<String>,
    requested_at: u64,
    render_started_at: u64,
    render_ended_at: u64,
    submitted_at: u64,
    write_started_at: u64,
    write_ended_at: u64,
    acknowledged_at: u64,
    frame_kind: String,
    byte_count: usize,
    byte_sha256: String,
    acknowledgement: RuntimeAckOutcome,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RuntimeAck {
    sequence: u64,
    revision: u64,
    cause_ids: Vec<String>,
    requested_at: u64,
    render_started_at: u64,
    render_ended_at: u64,
    submitted_at: u64,
    write_started_at: u64,
    write_ended_at: u64,
    acknowledged_at: u64,
    frame_kind: String,
    byte_count: usize,
    byte_sha256: String,
    outcome: RuntimeAckOutcome,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum RuntimeAckOutcome {
    Success,
    Failure { stage: String },
}

pub fn read_native_trace(path: &Path) -> Result<NativePresentationTrace, RunnerError> {
    let bytes = std::fs::read(path).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: format!("required native presentation sidecar: {error}"),
    })?;
    let raw: RuntimeTrace = serde_json::from_slice(&bytes).map_err(|error| RunnerError::Io {
        path: path.to_path_buf(),
        detail: format!("invalid native presentation sidecar: {error}"),
    })?;
    convert(raw)
}

fn convert(raw: RuntimeTrace) -> Result<NativePresentationTrace, RunnerError> {
    raw_integrity::validate(&raw)?;
    let causes = raw
        .causes
        .into_iter()
        .map(convert_cause)
        .collect::<Result<Vec<_>, _>>()?;
    let demands = raw.demands.into_iter().map(convert_demand).collect();
    let frames = raw.frames.into_iter().map(convert_frame).collect();
    let acknowledgements = raw.acknowledgements.into_iter().map(convert_ack).collect();
    Ok(NativePresentationTrace {
        trace_id: raw.trace_id,
        causes,
        demands,
        frames,
        acknowledgements,
        outcomes: raw
            .outcomes
            .into_iter()
            .map(convert_trace_outcome)
            .collect::<Result<Vec<_>, _>>()?,
        aggregates: raw.aggregates,
    })
}

fn convert_cause(value: RuntimeCause) -> Result<NativeCause, RunnerError> {
    let outcome = match value.outcome {
        RuntimeCauseOutcome::VisibleChange { cause_id, revision } => {
            let _ = (cause_id, revision);
            NativeCauseOutcome::VisibleChange
        }
        RuntimeCauseOutcome::NoVisibleChange {
            cause_id,
            closed_at,
        } => {
            let _ = (cause_id, closed_at);
            NativeCauseOutcome::NoVisibleChange
        }
        RuntimeCauseOutcome::Pending => return Err(invalid("native cause outcome is pending")),
        RuntimeCauseOutcome::ResyncRequired { .. } => {
            return Err(invalid("native cause outcome is resync_required"));
        }
    };
    Ok(NativeCause {
        cause_id: value.cause_id,
        interaction_id: value.interaction_id.map(InteractionId),
        received_at: PresentationTimestamp(value.received_at),
        kind: value.kind,
        resulting_revision: value.resulting_revision,
        outcome,
    })
}

fn convert_demand(value: RuntimeDemand) -> NativeDemand {
    NativeDemand {
        target_revision: value.target_revision,
        earliest_requested_at: PresentationTimestamp(value.earliest_requested_at),
        latest_requested_at: PresentationTimestamp(value.latest_requested_at),
        cause_ids: value.cause_ids,
        reason: value.reason,
        coalesced_request_count: value.coalesced_request_count,
    }
}

fn convert_frame(value: RuntimeFrame) -> NativeFrame {
    let _ = (
        value.write_started_at,
        value.write_ended_at,
        value.acknowledged_at,
        value.acknowledgement,
    );
    NativeFrame {
        sequence: value.sequence,
        revision: value.revision,
        cause_ids: value.cause_ids,
        requested_at: PresentationTimestamp(value.requested_at),
        render_started_at: PresentationTimestamp(value.render_started_at),
        render_ended_at: PresentationTimestamp(value.render_ended_at),
        submitted_at: PresentationTimestamp(value.submitted_at),
        frame_kind: value.frame_kind,
        byte_count: value.byte_count,
        byte_sha256: value.byte_sha256,
    }
}

fn convert_ack(value: RuntimeAck) -> NativeFrameAck {
    let _ = (
        value.cause_ids,
        value.requested_at,
        value.render_started_at,
        value.render_ended_at,
        value.submitted_at,
        value.frame_kind,
        value.byte_count,
    );
    NativeFrameAck {
        sequence: value.sequence,
        revision: value.revision,
        byte_sha256: value.byte_sha256,
        write_started_at: PresentationTimestamp(value.write_started_at),
        write_ended_at: PresentationTimestamp(value.write_ended_at),
        acknowledged_at: PresentationTimestamp(value.acknowledged_at),
        outcome: match value.outcome {
            RuntimeAckOutcome::Success => NativeAckOutcome::CompletedWrite,
            RuntimeAckOutcome::Failure { stage } => {
                let _ = stage;
                NativeAckOutcome::FailedWrite
            }
        },
    }
}

fn invalid(detail: &str) -> RunnerError {
    RunnerError::Process {
        adapter: crate::tui_fidelity::AdapterKind::Harness,
        detail: detail.to_owned(),
    }
}
