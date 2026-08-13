use std::path::Path;

use super::gates;
use super::helpers::{evidence, find_unique, read_json, verify_artifact};
use super::input_visibility::{visible_send_timestamps, NativeAcknowledgementOutcome};
use super::scheduling;
use super::types::{Presentation, Receipt, Run};
use super::{packet2_contract, AcceptanceProfile, AggregateError, Authority};
use crate::tui_fidelity_compare::ComparisonReceipt;
use crate::tui_fidelity_runner::{CleanupReceipt, PresentationMetricsKind};

pub(super) fn read(root: &Path, profile: AcceptanceProfile) -> Result<Run, AggregateError> {
    let receipt_path = find_unique(root, "receipt.json")?;
    let comparison_path = find_unique(root, "comparison.json")?;
    let cleanup_path = find_unique(root, "cleanup.json")?;
    let receipt: Receipt = read_json(&receipt_path)?;
    gates::reject_duplicates(&comparison_path)?;
    let comparison: ComparisonReceipt = read_json(&comparison_path)?;
    let cleanup: CleanupReceipt = read_json(&cleanup_path)?;
    if comparison.acceptance_profile != profile
        || !comparison.capture_succeeded
        || !gates::valid(&comparison, profile)
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
    let (external, native, links, sidecar) = match &candidate.presentation {
        Presentation::HarnessNative {
            external,
            native,
            links,
            scheduling_sidecar,
            ..
        } if native.aggregates.idle_redraws == 0 => {
            (external, native, links, scheduling_sidecar.as_ref())
        }
        _ => return evidence(root, "Harness native evidence or zero idle redraws missing"),
    };
    let Presentation::ExternalOnly {
        external: reference_external,
    } = &reference.presentation
    else {
        return evidence(root, "Grok is not external-only");
    };
    let observed_order = input_order(&external.actual_input_sends);
    if observed_order != input_order(&reference_external.actual_input_sends) {
        return evidence(root, "reference and candidate input order differs");
    }
    let active_window = external
        .actual_input_sends
        .first()
        .and_then(|send| send.sent_at)
        .zip(
            external
                .actual_input_sends
                .last()
                .and_then(|send| send.sent_at),
        );
    let contract = if profile == AcceptanceProfile::Packet2Scheduling {
        visible_send_timestamps(&external.actual_input_sends, native, root)?;
        let contract =
            packet2_contract::verify(&external.actual_input_sends, external, native, links, root)?;
        if !native
            .acknowledgements
            .iter()
            .any(|ack| ack.outcome == NativeAcknowledgementOutcome::CompletedWrite)
        {
            return evidence(root, "Harness native completed_write proof missing");
        }
        let sidecar = sidecar.ok_or_else(|| AggregateError::Evidence {
            path: root.to_path_buf(),
            detail: "missing Harness scheduling sidecar digest".into(),
        })?;
        scheduling::verify(sidecar, &observed_order)?;
        Some(contract)
    } else {
        None
    };
    let artifacts = verify_artifacts(&receipt)?;
    verify_bindings(&receipt, reference, candidate, root)?;
    let binding = &candidate.presentation_binding;
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
        input_order: observed_order,
        metrics,
        candidate_active_window: active_window,
        packet2_contract: contract,
        artifacts,
    })
}

fn input_order(sends: &[super::types::InputSend]) -> Vec<(usize, String)> {
    sends
        .iter()
        .map(|send| (send.action_ordinal, send.interaction_id.clone()))
        .collect()
}

fn verify_artifacts(receipt: &Receipt) -> Result<Vec<String>, AggregateError> {
    let mut hashes = Vec::new();
    for runtime in &receipt.runtimes {
        let external = match &runtime.presentation {
            Presentation::ExternalOnly { external }
            | Presentation::HarnessNative { external, .. } => external,
        };
        for artifact in [&external.raw_ansi, &external.observations_artifact] {
            verify_artifact(artifact)?;
            hashes.push(artifact.sha256.clone());
        }
        if let Presentation::HarnessNative {
            native_trace_artifact,
            ..
        } = &runtime.presentation
        {
            verify_artifact(native_trace_artifact)?;
            hashes.push(native_trace_artifact.sha256.clone());
        }
    }
    Ok(hashes)
}

fn verify_bindings(
    receipt: &Receipt,
    reference: &super::types::Runtime,
    candidate: &super::types::Runtime,
    root: &Path,
) -> Result<(), AggregateError> {
    let binding = &candidate.presentation_binding;
    let scenario_mismatch = binding.scenario_id != receipt.scenario_id;
    let schema_mismatch = binding.receipt_schema != receipt.schema_version;
    let measurement_mismatch =
        binding.measurement_kind != PresentationMetricsKind::ExternalPtyObserved;
    if scenario_mismatch || schema_mismatch || measurement_mismatch {
        return evidence(root, "receipt binding mismatch");
    }
    let other = &reference.presentation_binding;
    if other.receipt_schema != binding.receipt_schema
        || other.scenario_id != binding.scenario_id
        || other.action_schedule_sha256 != binding.action_schedule_sha256
        || other.motion_contract_sha256 != binding.motion_contract_sha256
        || other.observer_version != binding.observer_version
        || other.terminal_identity != binding.terminal_identity
        || other.measurement_kind != binding.measurement_kind
    {
        return evidence(root, "reference and candidate bindings differ");
    }
    Ok(())
}
