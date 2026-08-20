use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::tui_fidelity_compare::hash_bytes;

use super::{VerificationProfile, VerificationReceipt, VerificationStatus, VerifyError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompletionBindings {
    pub candidate_sha: String,
    pub authority_sha256: String,
    pub inventory_sha256: String,
    pub coverage_sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActiveCompletion {
    pub verification_receipt_path: PathBuf,
    pub verification_receipt_sha256: String,
    pub bindings: CompletionBindings,
}

#[derive(Deserialize)]
struct SealManifest {
    schema_version: String,
    candidate: String,
    records: Vec<SealRecord>,
}

#[derive(Deserialize)]
struct SealRecord {
    state: String,
}

pub fn validate_active_completion(
    receipt_path: &Path,
    expected: &CompletionBindings,
) -> Result<ActiveCompletion, VerifyError> {
    let bytes = fs::read(receipt_path).map_err(|error| VerifyError::Io {
        path: receipt_path.to_path_buf(),
        detail: error.to_string(),
    })?;
    let receipt: VerificationReceipt =
        serde_json::from_slice(&bytes).map_err(|error| VerifyError::Json(error.to_string()))?;
    if receipt.schema_version != "harness.tui-fidelity.verification.v1"
        || receipt.status != VerificationStatus::Passed
        || receipt.profile != VerificationProfile::All
        || !receipt.sealed
        || receipt.candidate_sha != expected.candidate_sha
        || receipt.authority_sha256 != expected.authority_sha256
        || receipt.inventory_sha256 != expected.inventory_sha256
        || receipt.coverage_sha256 != expected.coverage_sha256
    {
        return Err(VerifyError::Invalid(
            "receipt is not an active sealed verify-all result for the expected bindings"
                .to_owned(),
        ));
    }
    if receipt.key_count == 0
        || receipt.scheduler.passed != receipt.key_count
        || receipt.scheduler.failed != 0
        || receipt.scheduler.cancelled != 0
        || receipt.scheduler.skipped != 0
    {
        return Err(VerifyError::Invalid(
            "receipt scheduler does not prove every verification key passed".to_owned(),
        ));
    }
    let evidence_path = PathBuf::from(&receipt.evidence_path);
    if receipt_path != evidence_path.join("verification-receipt.json") {
        return Err(VerifyError::Invalid(
            "receipt path is not inside its sealed evidence directory".to_owned(),
        ));
    }
    let seal_path = evidence_path.join("seal-manifest.json");
    let seal_bytes = fs::read(&seal_path).map_err(|error| VerifyError::Io {
        path: seal_path,
        detail: error.to_string(),
    })?;
    let seal: SealManifest = serde_json::from_slice(&seal_bytes)
        .map_err(|error| VerifyError::Json(error.to_string()))?;
    if seal.schema_version != "harness.tui-fidelity.sealed-evidence.v1"
        || seal.candidate != expected.candidate_sha
        || seal.records.len() != receipt.key_count
        || seal.records.is_empty()
        || seal.records.iter().any(|record| record.state != "passed")
    {
        return Err(VerifyError::Invalid(
            "sealed evidence does not contain one passed record per verification key".to_owned(),
        ));
    }
    let receipt_sha256 =
        hash_bytes(&bytes).map_err(|error| VerifyError::Json(error.to_string()))?;
    Ok(ActiveCompletion {
        verification_receipt_path: receipt_path.to_path_buf(),
        verification_receipt_sha256: receipt_sha256,
        bindings: expected.clone(),
    })
}
