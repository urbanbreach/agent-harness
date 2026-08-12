use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::tui_fidelity_compare::{ComparisonReceipt, PresentationTimingMetrics};
use crate::tui_fidelity_runner::{CleanupReceipt, PresentationMetricsKind};

mod helpers;
use helpers::{evidence, find_unique, read_json, summarize, verify_artifact};

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
}

#[derive(Deserialize)]
struct Artifact {
    path: PathBuf,
    sha256: String,
}

#[derive(Deserialize)]
struct Native {
    aggregates: NativeAggregates,
}

#[derive(Deserialize)]
struct NativeAggregates {
    idle_redraws: u64,
}

pub fn aggregate(run_roots: &[PathBuf]) -> Result<AggregateSummary, AggregateError> {
    if run_roots.len() != RUN_COUNT {
        return Err(AggregateError::RunCount(run_roots.len()));
    }
    let mut runs = Vec::with_capacity(RUN_COUNT);
    for root in run_roots {
        runs.push(read_run(root)?);
    }
    let authority = runs[0].authority.clone();
    let input_order = runs[0].input_order.clone();
    for run in &runs {
        if run.authority != authority || run.input_order != input_order {
            return Err(AggregateError::MixedAuthority(run.root.clone()));
        }
    }
    summarize(authority, &runs)
}

struct Run {
    root: PathBuf,
    authority: Authority,
    input_order: Vec<(usize, String)>,
    metrics: crate::tui_fidelity_compare::PresentationComparisonMetrics,
    artifacts: Vec<String>,
}

fn read_run(root: &Path) -> Result<Run, AggregateError> {
    let receipt_path = find_unique(root, "receipt.json")?;
    let comparison_path = find_unique(root, "comparison.json")?;
    let cleanup_path = find_unique(root, "cleanup.json")?;
    let receipt: Receipt = read_json(&receipt_path)?;
    let comparison: ComparisonReceipt = read_json(&comparison_path)?;
    let cleanup: CleanupReceipt = read_json(&cleanup_path)?;
    if !comparison.capture_succeeded
        || !comparison.comparison_passed
        || comparison.gates.values().any(|gate| !gate.passed)
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
    let external = match &candidate.presentation {
        Presentation::HarnessNative {
            external, native, ..
        } if native.aggregates.idle_redraws == 0 => external,
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
    let reference_order = reference_external
        .actual_input_sends
        .iter()
        .map(|send| (send.action_ordinal, send.interaction_id.clone()))
        .collect::<Vec<_>>();
    if input_order != reference_order {
        return evidence(root, "reference and candidate input order differs");
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
        artifacts,
    })
}
