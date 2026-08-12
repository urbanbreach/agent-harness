use crate::tui_fidelity::Scenario;
use crate::tui_fidelity_runner::{DualRuntimeReceipt, RUNNER_RECEIPT_SCHEMA};

use super::super::error::ComparatorError;
use super::super::types::PresentationComparisonMetrics;
use super::runtime_pair;

pub fn validate(scenario: &Scenario, capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    if capture.schema_version != RUNNER_RECEIPT_SCHEMA {
        return Err(ComparatorError::Invalid {
            detail: "runner receipt schema is stale".to_owned(),
        });
    }
    let _ = runtime_pair(capture)?;
    let action_digest = hash_serialized(&scenario.actions)?;
    let motion_digest = hash_serialized(&scenario.motion_capture)?;
    for runtime in &capture.runtimes {
        crate::tui_fidelity_runner::validate_presentation_evidence(
            runtime.adapter,
            &runtime.presentation,
        )
        .map_err(|error| ComparatorError::Invalid {
            detail: format!("{} presentation: {error}", runtime.adapter.as_str()),
        })?;
        if runtime.presentation_binding.receipt_schema != capture.schema_version
            || runtime.presentation_binding.scenario_id != capture.scenario_id
            || runtime.presentation_binding.terminal_identity != capture.terminal_type
            || runtime.presentation_binding.action_schedule_sha256 != action_digest
            || runtime.presentation_binding.motion_contract_sha256 != motion_digest
        {
            return Err(ComparatorError::Invalid {
                detail: format!("{} presentation binding is stale", runtime.adapter.as_str()),
            });
        }
    }
    Ok(())
}

pub fn metrics(
    capture: &DualRuntimeReceipt,
) -> Result<PresentationComparisonMetrics, ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    super::super::presentation_timing::derive_comparison_presentation_timing(
        &reference.presentation,
        &candidate.presentation,
    )
}

pub fn timing(capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let metrics = metrics(capture)?;
    super::super::presentation_timing_gate::compare_presentation_timing(
        &metrics.reference,
        &metrics.candidate,
    )
}

pub fn motion(scenario: &Scenario, capture: &DualRuntimeReceipt) -> Result<(), ComparatorError> {
    let (reference, candidate) = runtime_pair(capture)?;
    super::super::ordered_motion::compare_ordered_motion(scenario, reference, candidate)
}

fn hash_serialized(value: &impl serde::Serialize) -> Result<String, ComparatorError> {
    serde_json::to_vec(value)
        .map_err(|error| ComparatorError::Invalid {
            detail: format!("presentation binding serialization: {error}"),
        })
        .and_then(|bytes| super::super::hashing::hash_bytes(&bytes))
}
