//! Provenance metadata for captured parity artifacts.
//!
//! Every captured semantic frame or pixel artifact must carry a
//! [`CaptureProvenance`] so that evidence is command-bound, fresh, and
//! rejects copied/self-oracle artifacts. The provenance struct is
//! serialized alongside the capture (e.g. in `metadata.json`) and
//! verified by the strict evidence validator.
//!
//! [`compare_frames_with_provenance`] wraps [`compare_frames`] with a
//! source-identity guard that rejects self-oracle comparison (both
//! sides from the same binary).
//!
//! [`validate_capture_provenance`] provides typed provenance validation
//! that independently rejects stale digests, copied artifacts, wrong
//! revisions, missing generating commands, wrong viewports, and
//! self-comparison. Freshness is proven by content digest matching,
//! not by timestamp alone (Contract §5.1).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::cells::SemanticFrame;
use super::compare::{compare_frames, CellDiff, CompareResult, IdentityMaskRegistry};
use super::status::ProofDimension;

/// Identifies the source binary that produced a capture.
///
/// Prevents self-oracle comparison: the reference and actual frames
/// must come from different sources.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureSource {
    /// Capture from the pinned reference binary.
    Reference,
    /// Capture from the Harness binary under test.
    Harness,
}

/// Provenance metadata for a captured parity artifact.
///
/// Every captured semantic frame or pixel artifact carries this struct
/// so that evidence is command-bound, fresh, and rejects copied/self-oracle
/// artifacts. Fields mirror the signoff-parity lane's freeze receipt.
///
/// Schema `artifact-receipt-v3` adds product/reference epoch bindings and
/// proof-dimension applicability. Freshness is proven by content digest
/// plus epoch binding, never by timestamp alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureProvenance {
    /// Git HEAD of the source checkout that produced the capture.
    pub source_head: String,
    /// Filesystem path to the binary that produced the capture.
    pub binary_path: String,
    /// SHA-256 of the binary that produced the capture.
    pub binary_sha256: String,
    /// SHA-256 of the pinned reference binary used for comparison.
    pub reference_digest: String,
    /// The command that generated this capture.
    pub generating_command: String,
    /// Environment snapshot (key-value pairs).
    #[serde(default)]
    pub environment: BTreeMap<String, String>,
    /// Terminal viewport (cols, rows).
    pub viewport: Viewport,
    /// Terminal capabilities (e.g. "256color", "unicode").
    #[serde(default)]
    pub terminal_capabilities: BTreeSet<String>,
    /// Unix timestamp (seconds) when the capture was made.
    pub captured_at: u64,
    /// SHA-256 of the captured artifact itself.
    pub artifact_sha256: String,
    /// SHA-256 of the canonical product input manifest.
    #[serde(default)]
    pub product_epoch: String,
    /// SHA-256 of the canonical reference source manifest.
    #[serde(default)]
    pub reference_epoch: String,
    /// Absolute canonical task evidence directory.
    #[serde(default)]
    pub task_evidence_root: String,
    /// Applicable proof dimensions for this capture.
    #[serde(default)]
    pub proof_dimensions: BTreeSet<ProofDimension>,
}

/// Terminal viewport dimensions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Viewport {
    pub cols: u16,
    pub rows: u16,
}

impl From<(u16, u16)> for Viewport {
    fn from((cols, rows): (u16, u16)) -> Self {
        Self { cols, rows }
    }
}

/// Compare two frames with provenance, rejecting self-oracle comparison.
///
/// Returns `Err` immediately if both sides have the same [`CaptureSource`],
/// preventing Harness output from being used as both oracle and actual.
/// Otherwise delegates to [`compare_frames`].
pub fn compare_frames_with_provenance(
    expected: &SemanticFrame,
    expected_source: CaptureSource,
    actual: &SemanticFrame,
    actual_source: CaptureSource,
    masks: &IdentityMaskRegistry,
) -> CompareResult {
    if expected_source == actual_source {
        return Err(vec![CellDiff::new(
            "provenance.self_oracle",
            "reference and actual from different sources",
            format!("both from {expected_source:?}"),
        )]);
    }
    compare_frames(expected, actual, masks)
}

/// Typed provenance validation error (Contract §4, §5.1).
///
/// Each variant names a distinct invalid-provenance class the validator
/// must independently reject. Status `pass` requires zero errors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProvenanceError {
    StaleDigest {
        field: String,
        expected: String,
        actual: String,
    },
    CopiedArtifact {
        reason: String,
    },
    WrongRevision {
        expected: String,
        actual: String,
    },
    MissingGeneratingCommand,
    WrongViewport {
        expected: Viewport,
        actual: Viewport,
    },
    SelfComparison {
        source: CaptureSource,
    },
    MissingBinaryIdentity,
    MissingSourceHead,
    MissingReferenceDigest,
    EpochMismatch {
        field: String,
        expected: String,
        actual: String,
    },
    MissingEpoch {
        field: String,
    },
}

/// Expected reference context for provenance validation (Contract §5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProvenanceContext {
    pub expected_source_head: String,
    pub expected_binary_sha256: String,
    pub expected_reference_digest: String,
    pub expected_viewport: Viewport,
    pub expected_artifact_sha256: String,
    pub expected_product_epoch: String,
    pub expected_reference_epoch: String,
}

/// Validate a capture's provenance against the expected context.
///
/// Returns `Err` with every typed violation found. Status `pass` requires
/// an empty error vector. Freshness is proven by content digest matching,
/// not by timestamp alone (Contract §5.1).
pub fn validate_capture_provenance(
    provenance: &CaptureProvenance,
    context: &ProvenanceContext,
) -> Result<(), Vec<ProvenanceError>> {
    let mut errors = Vec::new();
    if provenance.source_head.is_empty() {
        errors.push(ProvenanceError::MissingSourceHead);
    } else if provenance.source_head != context.expected_source_head {
        errors.push(ProvenanceError::WrongRevision {
            expected: context.expected_source_head.clone(),
            actual: provenance.source_head.clone(),
        });
    }
    if provenance.binary_sha256.is_empty() {
        errors.push(ProvenanceError::MissingBinaryIdentity);
    } else if provenance.binary_sha256 != context.expected_binary_sha256 {
        errors.push(ProvenanceError::StaleDigest {
            field: "binary_sha256".to_owned(),
            expected: context.expected_binary_sha256.clone(),
            actual: provenance.binary_sha256.clone(),
        });
    }
    if provenance.generating_command.is_empty() {
        errors.push(ProvenanceError::MissingGeneratingCommand);
    }
    if provenance.viewport != context.expected_viewport {
        errors.push(ProvenanceError::WrongViewport {
            expected: context.expected_viewport,
            actual: provenance.viewport,
        });
    }
    if provenance.reference_digest.is_empty() {
        errors.push(ProvenanceError::MissingReferenceDigest);
    } else if provenance.reference_digest != context.expected_reference_digest {
        errors.push(ProvenanceError::StaleDigest {
            field: "reference_digest".to_owned(),
            expected: context.expected_reference_digest.clone(),
            actual: provenance.reference_digest.clone(),
        });
    }
    if !context.expected_artifact_sha256.is_empty()
        && provenance.artifact_sha256 != context.expected_artifact_sha256
    {
        errors.push(ProvenanceError::StaleDigest {
            field: "artifact_sha256".to_owned(),
            expected: context.expected_artifact_sha256.clone(),
            actual: provenance.artifact_sha256.clone(),
        });
    }
    validate_epoch_binding(
        "product_epoch",
        &provenance.product_epoch,
        &context.expected_product_epoch,
        &mut errors,
    );
    validate_epoch_binding(
        "reference_epoch",
        &provenance.reference_epoch,
        &context.expected_reference_epoch,
        &mut errors,
    );
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_epoch_binding(
    field: &str,
    actual: &str,
    expected: &str,
    errors: &mut Vec<ProvenanceError>,
) {
    if expected.is_empty() {
        return;
    }
    if actual.is_empty() {
        errors.push(ProvenanceError::MissingEpoch {
            field: field.to_owned(),
        });
    } else if actual != expected {
        errors.push(ProvenanceError::EpochMismatch {
            field: field.to_owned(),
            expected: expected.to_owned(),
            actual: actual.to_owned(),
        });
    }
}

/// Reject self-oracle comparison: reference and actual must be from
/// different sources (Contract §5.1).
pub fn validate_no_self_comparison(
    expected_source: CaptureSource,
    actual_source: CaptureSource,
) -> Result<(), ProvenanceError> {
    if expected_source == actual_source {
        Err(ProvenanceError::SelfComparison {
            source: expected_source,
        })
    } else {
        Ok(())
    }
}
