use std::path::PathBuf;

use serde::Deserialize;

use super::input_visibility::Native;
use super::packet2_contract::Packet2Contract;
use crate::tui_fidelity_compare::PresentationComparisonMetrics;
use crate::tui_fidelity_runner::PresentationMetricsKind;

#[derive(Deserialize)]
pub(super) struct Receipt {
    pub(super) schema_version: String,
    pub(super) scenario_id: String,
    pub(super) runtimes: Vec<Runtime>,
}

#[derive(Deserialize)]
pub(super) struct Runtime {
    pub(super) adapter: String,
    pub(super) binary: Binary,
    pub(super) presentation: Presentation,
    pub(super) presentation_binding: Binding,
}

#[derive(Deserialize)]
pub(super) struct Binary {
    pub(super) sha256: String,
}

#[derive(Deserialize)]
pub(super) struct Binding {
    pub(super) receipt_schema: String,
    pub(super) scenario_id: String,
    pub(super) action_schedule_sha256: String,
    pub(super) motion_contract_sha256: String,
    pub(super) observer_version: String,
    pub(super) terminal_identity: String,
    pub(super) measurement_kind: PresentationMetricsKind,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Presentation {
    ExternalOnly {
        external: External,
    },
    HarnessNative {
        external: External,
        native: Native,
        #[serde(default)]
        links: Vec<PresentationLink>,
        native_trace_artifact: Artifact,
        #[serde(default)]
        scheduling_sidecar: Option<Artifact>,
    },
}

#[derive(Deserialize)]
pub(super) struct External {
    pub(super) actual_input_sends: Vec<InputSend>,
    #[serde(default)]
    pub(super) observations: Vec<ExternalObservation>,
    #[serde(default)]
    pub(super) raw_reads: Vec<RawRead>,
    pub(super) raw_ansi: Artifact,
    pub(super) observations_artifact: Artifact,
}

#[derive(Deserialize)]
pub(super) struct ExternalObservation {
    pub(super) observed_at: u64,
    pub(super) raw_read_ordinals: Vec<usize>,
}

#[derive(Deserialize)]
pub(super) struct RawRead {
    pub(super) byte_len: u64,
    pub(super) sha256: String,
}

#[derive(Deserialize)]
pub(super) struct PresentationLink {
    pub(super) frame_sequence: u64,
    pub(super) byte_sha256: String,
    pub(super) stream_offset: u64,
}

#[derive(Deserialize)]
pub(super) struct InputSend {
    pub(super) interaction_id: String,
    pub(super) action_ordinal: usize,
    #[serde(default)]
    pub(super) sent_at: Option<u64>,
}

#[derive(Deserialize)]
pub(super) struct Artifact {
    pub(super) path: PathBuf,
    pub(super) sha256: String,
}

pub(super) struct Run {
    pub(super) root: PathBuf,
    pub(super) authority: super::Authority,
    pub(super) input_order: Vec<(usize, String)>,
    pub(super) metrics: PresentationComparisonMetrics,
    pub(super) candidate_active_window: Option<(u64, u64)>,
    pub(super) packet2_contract: Option<Packet2Contract>,
    pub(super) artifacts: Vec<String>,
}
