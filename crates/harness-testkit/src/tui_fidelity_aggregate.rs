use std::path::{Path, PathBuf};

use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tui_fidelity_compare::{
    AcceptanceProfile, ComparisonReceipt, PresentationTimingMetrics,
};
use crate::tui_fidelity_runner::{CleanupReceipt, PresentationMetricsKind};

mod helpers;
mod input_visibility;
use helpers::{evidence, find_unique, read_json, summarize, verify_artifact};
use input_visibility::{visible_send_timestamps, Native, NativeAcknowledgementOutcome};

const RUN_COUNT: usize = 5;

#[derive(Debug, thiserror::Error)]
pub enum AggregateError {
    #[error("exactly five run roots are required, got {0}")]
    RunCount(usize),
    #[error("{path}: {detail}")]
    Evidence { path: PathBuf, detail: String },
    #[error("run authority differs at {0}")]
    MixedAuthority(PathBuf),
    #[error("aggregate threshold failed: {0}")]
    Threshold(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AggregateSummary {
    pub schema_version: &'static str,
    pub run_count: usize,
    pub authority: Authority,
    pub reference_external_p95_micros: u64,
    pub candidate_external_p95_micros: u64,
    pub candidate_native_receive_to_flush_p95_micros: u64,
    pub candidate_interval_p95_micros: u64,
    pub candidate_interval_max_micros: u64,
    pub coalesced_requests: u64,
    pub queue_saturation: u64,
    pub resyncs: u64,
    pub full_repaints: u64,
    pub idle_redraws: u64,
    pub artifact_sha256: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authority {
    scenario_id: String,
    receipt_schema: String,
    comparison_schema: String,
    reference_sha256: String,
    candidate_sha256: String,
    action_schedule_sha256: String,
    motion_contract_sha256: String,
    observer_version: String,
    terminal_identity: String,
}

#[derive(Deserialize)]
struct Receipt {
    schema_version: String,
    scenario_id: String,
    runtimes: Vec<Runtime>,
}

#[derive(Deserialize)]
struct Runtime {
    adapter: String,
    binary: Binary,
    presentation: Presentation,
    presentation_binding: Binding,
}

#[derive(Deserialize)]
struct Binary {
    sha256: String,
}

#[derive(Deserialize)]
struct Binding {
    receipt_schema: String,
    scenario_id: String,
    action_schedule_sha256: String,
    motion_contract_sha256: String,
    observer_version: String,
    terminal_identity: String,
    measurement_kind: PresentationMetricsKind,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Presentation {
    ExternalOnly {
        external: External,
    },
    HarnessNative {
        external: External,
        native: Native,
        native_trace_artifact: Artifact,
        #[serde(default)]
        scheduling_sidecar: Option<Artifact>,
    },
}

#[derive(Deserialize)]
struct External {
    actual_input_sends: Vec<InputSend>,
    raw_ansi: Artifact,
    observations_artifact: Artifact,
}

#[derive(Deserialize)]
struct InputSend {
    interaction_id: String,
    action_ordinal: usize,
    #[serde(default)]
    sent_at: Option<u64>,
}

#[derive(Deserialize)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}

pub fn aggregate(run_roots: &[PathBuf]) -> Result<AggregateSummary, AggregateError> {
    aggregate_with_profile(run_roots, AcceptanceProfile::FullParity)
}

pub fn aggregate_with_profile(
    run_roots: &[PathBuf],
    profile: AcceptanceProfile,
) -> Result<AggregateSummary, AggregateError> {
    if run_roots.len() != RUN_COUNT {
        return Err(AggregateError::RunCount(run_roots.len()));
    }
    let mut runs = Vec::with_capacity(RUN_COUNT);
    for root in run_roots {
        runs.push(read_run(root, profile)?);
    }
    let authority = runs[0].authority.clone();
    let input_order = runs[0].input_order.clone();
    for run in &runs {
        if run.authority != authority || run.input_order != input_order {
            return Err(AggregateError::MixedAuthority(run.root.clone()));
        }
    }
    summarize(authority, &runs, profile)
}

struct Run {
    root: PathBuf,
    authority: Authority,
    input_order: Vec<(usize, String)>,
    metrics: crate::tui_fidelity_compare::PresentationComparisonMetrics,
    candidate_active_window: Option<(u64, u64)>,
    candidate_send_timestamps: Vec<u64>,
    artifacts: Vec<String>,
}

fn read_run(root: &Path, profile: AcceptanceProfile) -> Result<Run, AggregateError> {
    let receipt_path = find_unique(root, "receipt.json")?;
    let comparison_path = find_unique(root, "comparison.json")?;
    let cleanup_path = find_unique(root, "cleanup.json")?;
    let receipt: Receipt = read_json(&receipt_path)?;
    reject_duplicate_gates(&comparison_path)?;
    let comparison: ComparisonReceipt = read_json(&comparison_path)?;
    let cleanup: CleanupReceipt = read_json(&cleanup_path)?;
    if comparison.acceptance_profile != profile
        || !comparison.capture_succeeded
        || !valid_gates(&comparison, profile)
        || cleanup.status != "clean"
        || !cleanup.surviving_pids.is_empty()
        || !cleanup.cleanup_errors.is_empty()
    {
        return evidence(root, "comparison or cleanup did not pass");
    }
    let metrics = comparison
        .presentation
        .ok_or_else(|| AggregateError::Evidence {
            path: comparison_path,
            detail: "missing presentation metrics".into(),
        })?;
    let reference = receipt.runtimes.iter().find(|run| run.adapter == "grok");
    let candidate = receipt.runtimes.iter().find(|run| run.adapter == "harness");
    let (Some(reference), Some(candidate)) = (reference, candidate) else {
        return evidence(root, "missing Grok or Harness runtime");
    };
    let (external, native, scheduling_sidecar) = match &candidate.presentation {
        Presentation::HarnessNative {
            external,
            native,
            scheduling_sidecar,
            ..
        } if native.aggregates.idle_redraws == 0 => (external, native, scheduling_sidecar.as_ref()),
        _ => return evidence(root, "Harness native evidence or zero idle redraws missing"),
    };
    let Presentation::ExternalOnly {
        external: reference_external,
    } = &reference.presentation
    else {
        return evidence(root, "Grok is not external-only");
    };
    let input_order = external
        .actual_input_sends
        .iter()
        .map(|send| (send.action_ordinal, send.interaction_id.clone()))
        .collect::<Vec<_>>();
    let candidate_active_window = external
        .actual_input_sends
        .first()
        .and_then(|send| send.sent_at)
        .zip(
            external
                .actual_input_sends
                .last()
                .and_then(|send| send.sent_at),
        );
    let candidate_send_timestamps = if profile == AcceptanceProfile::Packet2Scheduling {
        visible_send_timestamps(&external.actual_input_sends, native, root)?
    } else {
        external
            .actual_input_sends
            .iter()
            .filter_map(|send| send.sent_at)
            .collect()
    };
    let reference_order = reference_external
        .actual_input_sends
        .iter()
        .map(|send| (send.action_ordinal, send.interaction_id.clone()))
        .collect::<Vec<_>>();
    if input_order != reference_order {
        return evidence(root, "reference and candidate input order differs");
    }
    if profile == AcceptanceProfile::Packet2Scheduling {
        if !native
            .acknowledgements
            .iter()
            .any(|ack| ack.outcome == NativeAcknowledgementOutcome::CompletedWrite)
        {
            return evidence(root, "Harness native completed_write proof missing");
        }
        let artifact = scheduling_sidecar.ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "missing Harness scheduling sidecar digest".into(),
        })?;
        verify_scheduling_sidecar(artifact, &input_order)?;
    }
    let mut artifacts = Vec::new();
    for runtime in &receipt.runtimes {
        let external = match &runtime.presentation {
            Presentation::ExternalOnly { external }
            | Presentation::HarnessNative { external, .. } => external,
        };
        for artifact in [&external.raw_ansi, &external.observations_artifact] {
            verify_artifact(artifact)?;
            artifacts.push(artifact.sha256.clone());
        }
        if let Presentation::HarnessNative {
            native_trace_artifact,
            ..
        } = &runtime.presentation
        {
            verify_artifact(native_trace_artifact)?;
            artifacts.push(native_trace_artifact.sha256.clone());
        }
    }
    let binding = &candidate.presentation_binding;
    let scenario_mismatch = binding.scenario_id != receipt.scenario_id;
    let schema_mismatch = binding.receipt_schema != receipt.schema_version;
    let measurement_mismatch =
        binding.measurement_kind != PresentationMetricsKind::ExternalPtyObserved;
    if scenario_mismatch || schema_mismatch || measurement_mismatch {
        return evidence(root, "receipt binding mismatch");
    }
    let reference_binding = &reference.presentation_binding;
    if reference_binding.receipt_schema != binding.receipt_schema
        || reference_binding.scenario_id != binding.scenario_id
        || reference_binding.action_schedule_sha256 != binding.action_schedule_sha256
        || reference_binding.motion_contract_sha256 != binding.motion_contract_sha256
        || reference_binding.observer_version != binding.observer_version
        || reference_binding.terminal_identity != binding.terminal_identity
        || reference_binding.measurement_kind != binding.measurement_kind
    {
        return evidence(root, "reference and candidate bindings differ");
    }
    Ok(Run {
        root: root.to_path_buf(),
        authority: Authority {
            scenario_id: receipt.scenario_id,
            receipt_schema: receipt.schema_version,
            comparison_schema: comparison.schema_version,
            reference_sha256: reference.binary.sha256.clone(),
            candidate_sha256: candidate.binary.sha256.clone(),
            action_schedule_sha256: binding.action_schedule_sha256.clone(),
            motion_contract_sha256: binding.motion_contract_sha256.clone(),
            observer_version: binding.observer_version.clone(),
            terminal_identity: binding.terminal_identity.clone(),
        },
        input_order,
        metrics,
        candidate_active_window,
        candidate_send_timestamps,
        artifacts,
    })
}

fn valid_gates(comparison: &ComparisonReceipt, profile: AcceptanceProfile) -> bool {
    const ALL: [&str; 9] = [
        "presentation",
        "semantic_cell",
        "pixel",
        "motion",
        "timing",
        "provenance",
        "checkpoint",
        "exit",
        "cleanup",
    ];
    const PACKET2_REQUIRED: [&str; 5] = [
        "presentation",
        "provenance",
        "checkpoint",
        "exit",
        "cleanup",
    ];
    comparison.gates.len() == ALL.len()
        && ALL.iter().all(|name| comparison.gates.contains_key(*name))
        && match profile {
            AcceptanceProfile::FullParity => ALL
                .iter()
                .all(|name| comparison.gates.get(*name).is_some_and(|gate| gate.passed)),
            AcceptanceProfile::Packet2Scheduling => PACKET2_REQUIRED
                .iter()
                .all(|name| comparison.gates.get(*name).is_some_and(|gate| gate.passed)),
        }
}

#[derive(Deserialize)]
struct GateEnvelope {
    #[serde(deserialize_with = "unique_gate_map")]
    gates: (),
}

fn reject_duplicate_gates(path: &Path) -> Result<(), AggregateError> {
    let bytes = std::fs::read(path).map_err(|error| AggregateError::Evidence {
        path: path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let _: GateEnvelope =
        serde_json::from_slice(&bytes).map_err(|error| AggregateError::Evidence {
            path: path.to_path_buf(),
            detail: format!("invalid or duplicate comparison gate: {error}"),
        })?;
    Ok(())
}

fn unique_gate_map<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct UniqueGateVisitor;

    impl<'de> Visitor<'de> for UniqueGateVisitor {
        type Value = ();

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a comparison gate map with unique names")
        }

        fn visit_map<M>(self, mut map: M) -> Result<(), M::Error>
        where
            M: MapAccess<'de>,
        {
            let mut names = std::collections::BTreeSet::new();
            while let Some(name) = map.next_key::<String>()? {
                if !names.insert(name.clone()) {
                    return Err(serde::de::Error::custom(format!("duplicate gate `{name}`")));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }

    deserializer.deserialize_map(UniqueGateVisitor)
}

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

fn verify_scheduling_sidecar(
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
