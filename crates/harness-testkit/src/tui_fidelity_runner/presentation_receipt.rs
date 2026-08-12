use serde::{Deserialize, Serialize};

use crate::parity::SemanticFrame;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct InteractionId(pub String);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PresentationTimestamp(pub u64);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentationClock {
    pub unit: ClockUnit,
    pub epoch_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClockUnit {
    MonotonicMicroseconds,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActualInputSend {
    pub interaction_id: InteractionId,
    pub action_ordinal: usize,
    pub scheduled_at: PresentationTimestamp,
    pub sent_at: PresentationTimestamp,
    pub transport_drained_at: Option<PresentationTimestamp>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecoderState {
    Complete,
    AwaitingMore,
    Truncated,
    Malformed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawPtyRead {
    pub read_completed_at: PresentationTimestamp,
    pub byte_len: usize,
    pub sha256: String,
    pub decoder_state: DecoderState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservationKind {
    SynchronizedUpdateComplete,
    ReadCompletionDecode,
    StableRepeat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedSemanticObservation {
    pub observation_ordinal: usize,
    pub observed_at: PresentationTimestamp,
    pub kind: ObservationKind,
    pub decoder_state: DecoderState,
    pub raw_read_ordinals: Vec<usize>,
    pub frame: SemanticFrame,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InteractionObservation {
    pub interaction_id: InteractionId,
    pub first_changed_observation: Option<usize>,
    pub diagnostic: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresentationMetricsKind {
    ExternalPtyObserved,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExternalPresentationEvidence {
    pub clock: PresentationClock,
    pub actual_input_sends: Vec<ActualInputSend>,
    pub raw_reads: Vec<RawPtyRead>,
    pub observations: Vec<TimedSemanticObservation>,
    pub interaction_observations: Vec<InteractionObservation>,
    pub raw_ansi: super::types::ArtifactDigest,
    pub observations_artifact: super::types::ArtifactDigest,
    pub metrics_kind: PresentationMetricsKind,
    pub native_visual_observed_at: Option<PresentationTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeExternalLink {
    pub frame_sequence: u64,
    pub byte_sha256: String,
    pub stream_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCause {
    pub cause_id: String,
    pub interaction_id: Option<InteractionId>,
    pub received_at: PresentationTimestamp,
    pub kind: String,
    pub resulting_revision: Option<u64>,
    pub outcome: NativeCauseOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeCauseOutcome {
    VisibleChange,
    NoVisibleChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeDemand {
    pub target_revision: u64,
    pub earliest_requested_at: PresentationTimestamp,
    pub latest_requested_at: PresentationTimestamp,
    pub cause_ids: Vec<String>,
    pub reason: String,
    pub coalesced_request_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFrame {
    pub sequence: u64,
    pub revision: u64,
    pub cause_ids: Vec<String>,
    pub requested_at: PresentationTimestamp,
    pub render_started_at: PresentationTimestamp,
    pub render_ended_at: PresentationTimestamp,
    pub submitted_at: PresentationTimestamp,
    pub frame_kind: String,
    pub byte_count: usize,
    pub byte_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeFrameAck {
    pub sequence: u64,
    pub revision: u64,
    pub byte_sha256: String,
    pub write_started_at: PresentationTimestamp,
    pub write_ended_at: PresentationTimestamp,
    pub acknowledged_at: PresentationTimestamp,
    pub outcome: NativeAckOutcome,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeAckOutcome {
    CompletedWrite,
    FailedWrite,
    ResyncRequired,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePresentationOutcome {
    pub cause_id: Option<String>,
    pub kind: String,
    pub closed_at: Option<PresentationTimestamp>,
    pub rejected_revision: Option<u64>,
    pub replacement_revision: Option<u64>,
    pub recorded_at: Option<PresentationTimestamp>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePresentationAggregates {
    pub coalesced_requests: u64,
    pub queue_saturation: u64,
    pub resyncs: u64,
    pub full_repaints: u64,
    pub bytes_written: u64,
    pub idle_redraws: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativePresentationTrace {
    pub trace_id: String,
    pub causes: Vec<NativeCause>,
    pub demands: Vec<NativeDemand>,
    pub frames: Vec<NativeFrame>,
    pub acknowledgements: Vec<NativeFrameAck>,
    pub outcomes: Vec<NativePresentationOutcome>,
    pub aggregates: NativePresentationAggregates,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentationEvidence {
    ExternalOnly {
        external: ExternalPresentationEvidence,
    },
    HarnessNative {
        external: ExternalPresentationEvidence,
        native: Box<NativePresentationTrace>,
        native_trace_artifact: super::types::ArtifactDigest,
        links: Vec<NativeExternalLink>,
    },
}
