use std::path::Path;

use super::{
    ArtifactReceipt, AuthMode, JourneyReceipt, ProviderMode, ValidationOutcome, ValidationResult,
    ARTIFACT_RECEIPT_SCHEMA_VERSION, SECRET_PATTERNS,
};

pub(super) fn validate_artifact_receipt(receipt: &ArtifactReceipt) -> ValidationResult {
    let mut missing = Vec::new();
    let mut rejected = Vec::new();

    required_fields(receipt, &mut missing);
    rejected_fields(receipt, &mut rejected);

    let secret_scan_clean = receipt.secret_scan.clean && receipt.secret_scan.findings.is_empty();
    if !secret_scan_clean {
        rejected.push("secret_scan".to_owned());
    }

    validation_result(missing, rejected, secret_scan_clean)
}

pub(super) fn validate_journey_receipt(journey: &JourneyReceipt) -> ValidationResult {
    let mut result = validate_artifact_receipt(&journey.artifact);

    if journey.journey_id.is_empty() {
        result.required_fields_missing.push("journey_id".to_owned());
    }
    if journey.provider_mode != journey.artifact.provider_mode {
        result.rejected_fields.push("provider_mode".to_owned());
    }
    if journey.auth_mode != journey.artifact.auth_mode {
        result.rejected_fields.push("auth_mode".to_owned());
    }
    if !result.required_fields_missing.is_empty() || !result.rejected_fields.is_empty() {
        result.outcome = ValidationOutcome::Fail;
    }

    result
}

fn required_fields(receipt: &ArtifactReceipt, missing: &mut Vec<String>) {
    for (field, value) in [
        ("binary_digest", receipt.binary_digest.as_str()),
        ("source_revision", receipt.source_revision.as_str()),
        ("command", receipt.command.as_str()),
        ("workspace_before", receipt.workspace_before.digest.as_str()),
        ("workspace_after", receipt.workspace_after.digest.as_str()),
        ("isolation_root", receipt.isolation_root.as_str()),
        ("owner", receipt.owner.as_str()),
        (
            "candidate.source_revision",
            receipt.candidate.source_revision.as_str(),
        ),
        (
            "candidate.binary_digest",
            receipt.candidate.binary_digest.as_str(),
        ),
        ("runner.path", receipt.runner.path.as_str()),
        ("runner.sha256", receipt.runner.sha256.as_str()),
        ("runner.version", receipt.runner.version.as_str()),
        ("runner.permissions", receipt.runner.permissions.as_str()),
        ("evidence.attempt_id", receipt.evidence.attempt_id.as_str()),
        ("evidence.root", receipt.evidence.root.as_str()),
        (
            "evidence.artifact_path",
            receipt.evidence.artifact_path.as_str(),
        ),
        (
            "evidence.artifact_sha256",
            receipt.evidence.artifact_sha256.as_str(),
        ),
    ] {
        if value.is_empty() {
            missing.push(field.to_owned());
        }
    }
    if receipt.provider_mode == ProviderMode::Unknown {
        missing.push("provider_mode".to_owned());
    }
    if receipt.auth_mode == AuthMode::Unknown {
        missing.push("auth_mode".to_owned());
    }
    if receipt.teardown.removed_paths.is_empty() && !receipt.teardown.workspace_restored {
        missing.push("teardown".to_owned());
    }
    if receipt.secret_scan.patterns_checked.is_empty() {
        missing.push("secret_scan".to_owned());
    }
    if receipt.evidence.task_id == 0 {
        missing.push("evidence.task_id".to_owned());
    }
    if !receipt.evidence.fresh_root {
        missing.push("evidence.fresh_root".to_owned());
    }
    if receipt.reference.path.is_empty() {
        missing.push("reference.path".to_owned());
    }
    if receipt.reference.sha256.is_empty() {
        missing.push("reference.sha256".to_owned());
    }
    if receipt.epoch.product_epoch.is_empty() {
        missing.push("epoch.product_epoch".to_owned());
    }
    if receipt.epoch.reference_epoch.is_empty() {
        missing.push("epoch.reference_epoch".to_owned());
    }
    if receipt.proof_dimensions.is_empty() {
        missing.push("proof_dimensions".to_owned());
    }
}

fn rejected_fields(receipt: &ArtifactReceipt, rejected: &mut Vec<String>) {
    if !is_absolute_path(&receipt.isolation_root) {
        rejected.push("isolation_root".to_owned());
    }
    if receipt.teardown.exit_code != 0 {
        rejected.push("teardown".to_owned());
    }
    if !is_canonical_task_root(
        &receipt.evidence.root,
        &receipt.evidence.attempt_id,
        receipt.evidence.task_id,
    ) || !is_artifact_in_root(&receipt.evidence.root, &receipt.evidence.artifact_path)
    {
        rejected.push("evidence".to_owned());
    }
    if !is_absolute_path(&receipt.runner.path)
        || receipt.runner.sha256 != receipt.binary_digest
        || !receipt.command.contains(&receipt.runner.path)
    {
        rejected.push("runner".to_owned());
    }
    if receipt.candidate.source_revision != receipt.source_revision
        || receipt.candidate.binary_digest != receipt.binary_digest
    {
        rejected.push("candidate".to_owned());
    }

    if !receipt.reference.path.is_empty() && !is_absolute_path(&receipt.reference.path) {
        rejected.push("reference".to_owned());
    }

    for (field, value) in [
        ("binary_digest", receipt.binary_digest.as_str()),
        ("source_revision", receipt.source_revision.as_str()),
        ("command", receipt.command.as_str()),
        ("isolation_root", receipt.isolation_root.as_str()),
        ("workspace_before", receipt.workspace_before.digest.as_str()),
        ("workspace_after", receipt.workspace_after.digest.as_str()),
        ("owner", receipt.owner.as_str()),
        ("candidate", receipt.candidate.source_revision.as_str()),
        ("candidate", receipt.candidate.binary_digest.as_str()),
        ("runner", receipt.runner.path.as_str()),
        ("runner", receipt.runner.sha256.as_str()),
        ("runner", receipt.runner.version.as_str()),
        ("evidence", receipt.evidence.root.as_str()),
        ("evidence", receipt.evidence.artifact_path.as_str()),
        ("evidence", receipt.evidence.artifact_sha256.as_str()),
        ("reference", receipt.reference.path.as_str()),
        ("reference", receipt.reference.sha256.as_str()),
        ("epoch", receipt.epoch.product_epoch.as_str()),
        ("epoch", receipt.epoch.reference_epoch.as_str()),
    ] {
        if contains_secret(value)
            && !rejected
                .iter()
                .any(|rejected_field| rejected_field == field)
        {
            rejected.push(field.to_owned());
        }
    }
}

fn validation_result(
    missing: Vec<String>,
    rejected: Vec<String>,
    secret_scan_clean: bool,
) -> ValidationResult {
    let outcome = if missing.is_empty() && rejected.is_empty() {
        ValidationOutcome::Pass
    } else {
        ValidationOutcome::Fail
    };
    ValidationResult {
        outcome,
        required_fields_missing: missing,
        rejected_fields: rejected,
        secret_scan_clean,
        schema_version: ARTIFACT_RECEIPT_SCHEMA_VERSION.to_owned(),
    }
}

fn is_absolute_path(value: &str) -> bool {
    Path::new(value).is_absolute()
}

fn is_canonical_task_root(root: &str, attempt_id: &str, task_id: u32) -> bool {
    let path = Path::new(root);
    let Some(task_dir) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(attempt_dir) = path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    path.is_absolute()
        && task_dir == format!("task-{task_id}")
        && attempt_dir == attempt_id
        && attempt_id.starts_with("attempt-")
}

fn is_artifact_in_root(root: &str, artifact_path: &str) -> bool {
    let root = Path::new(root);
    let artifact = Path::new(artifact_path);
    artifact.is_absolute() && artifact.starts_with(root) && artifact != root
}

fn contains_secret(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    SECRET_PATTERNS
        .iter()
        .filter(|pattern| **pattern != "sk-" && **pattern != "sk_")
        .any(|pattern| lower.contains(&pattern.to_ascii_lowercase()))
        || lower.starts_with("sk-")
        || lower.starts_with("sk_")
        || lower.contains("=sk-")
        || lower.contains("=sk_")
}
