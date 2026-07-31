//! Typed evidence-layer and proof-dimension completeness and status validation.
//!
//! Status `pass` requires every applicable evidence layer (legacy L0-L6) or
//! proof dimension (P0-P9) to be present and current-run provenance to be
//! valid. Layer applicability depends on row kind: visual rows need all,
//! journeys need a subset, and terminal capability rows need the lower layers.
//!
//! The P0-P9 proof dimensions supersede L0-L6 (schema `artifact-receipt-v3`).
//! Migration mapping: L0→P0, L1→P1, L2→P2, L3→P3, L4→P4, L5→P5, L6→P9.
//! P6 (rejection), P7 (lifecycle), P8 (external) are new dimensions with no
//! legacy equivalent.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::artifact_schema::{ArtifactReceipt, ValidationOutcome};

/// Evidence layers (Contract §5.2, legacy — retained for migration).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EvidenceLayer {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
    L6,
}

impl EvidenceLayer {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceLayer::L0 => "L0",
            EvidenceLayer::L1 => "L1",
            EvidenceLayer::L2 => "L2",
            EvidenceLayer::L3 => "L3",
            EvidenceLayer::L4 => "L4",
            EvidenceLayer::L5 => "L5",
            EvidenceLayer::L6 => "L6",
        }
    }

    /// Migrate a legacy evidence layer to its proof dimension equivalent.
    ///
    /// L0→P0, L1→P1, L2→P2, L3→P3, L4→P4, L5→P5, L6→P9.
    pub const fn to_proof_dimension(self) -> ProofDimension {
        match self {
            EvidenceLayer::L0 => ProofDimension::P0,
            EvidenceLayer::L1 => ProofDimension::P1,
            EvidenceLayer::L2 => ProofDimension::P2,
            EvidenceLayer::L3 => ProofDimension::P3,
            EvidenceLayer::L4 => ProofDimension::P4,
            EvidenceLayer::L5 => ProofDimension::P5,
            EvidenceLayer::L6 => ProofDimension::P9,
        }
    }
}

// ---------------------------------------------------------------------------
// Proof dimensions (P0-P9, schema artifact-receipt-v3)
// ---------------------------------------------------------------------------

/// Proof dimensions for clean-room parity evidence (P0-P9).
///
/// These supersede the legacy L0-L6 evidence layers. P6-P8 are new
/// dimensions with no legacy equivalent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProofDimension {
    /// P0 inventory: behavior id, reference source paths/symbols, Harness
    /// owner, disposition, trigger, focus, viewport, dependencies.
    P0,
    /// P1 contract: independently authored failing differential contract.
    P1,
    /// P2 owner: compiled public-surface owner call plus observable external
    /// postcondition.
    P2,
    /// P3 terminal: exact input trace, PTY bytes, semantic cells, cursor,
    /// alternate-screen state, focus owner.
    P3,
    /// P4 raster: settled reference/candidate PNGs and zero unapproved RGBA
    /// differences.
    P4,
    /// P5 motion: ordered frames, tick timestamps, settle dwell,
    /// scroll/resize/cancel/animation timing.
    P5,
    /// P6 rejection: stale/copy/self-oracle/wrong-binary/secret/mask-expansion/
    /// owner-bypass mutations fail closed.
    P6,
    /// P7 lifecycle: restart, persistence, error, cancel, recovery, and
    /// teardown receipts.
    P7,
    /// P8 external: live provider/native terminal/clipboard proof when
    /// required; unavailable environment is `blocked`.
    P8,
    /// P9 review: F1-F4 independent approvals plus terminal Oracle approval.
    P9,
}

impl ProofDimension {
    /// Canonical string representation.
    pub const fn as_str(self) -> &'static str {
        match self {
            ProofDimension::P0 => "P0",
            ProofDimension::P1 => "P1",
            ProofDimension::P2 => "P2",
            ProofDimension::P3 => "P3",
            ProofDimension::P4 => "P4",
            ProofDimension::P5 => "P5",
            ProofDimension::P6 => "P6",
            ProofDimension::P7 => "P7",
            ProofDimension::P8 => "P8",
            ProofDimension::P9 => "P9",
        }
    }

    /// All ten proof dimensions in order.
    pub const fn all() -> [ProofDimension; 10] {
        [
            ProofDimension::P0,
            ProofDimension::P1,
            ProofDimension::P2,
            ProofDimension::P3,
            ProofDimension::P4,
            ProofDimension::P5,
            ProofDimension::P6,
            ProofDimension::P7,
            ProofDimension::P8,
            ProofDimension::P9,
        ]
    }
}

impl fmt::Display for ProofDimension {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Result of checking layer completeness for a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerCompleteness {
    pub applicable_layers: BTreeSet<EvidenceLayer>,
    pub present_layers: BTreeSet<EvidenceLayer>,
    pub missing_layers: BTreeSet<EvidenceLayer>,
    pub is_complete: bool,
}

/// Result of checking proof-dimension completeness for a row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DimensionCompleteness {
    pub applicable_dimensions: BTreeSet<ProofDimension>,
    pub present_dimensions: BTreeSet<ProofDimension>,
    pub missing_dimensions: BTreeSet<ProofDimension>,
    pub is_complete: bool,
}

/// A reason a status `pass` claim is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PassStatusError {
    /// One or more required evidence layers are absent.
    MissingEvidence(BTreeSet<EvidenceLayer>),
    /// One or more required proof dimensions are absent.
    MissingDimensions(BTreeSet<ProofDimension>),
    /// The receipt has a secret, provenance, owner, or runner contradiction.
    InvalidReceipt,
}

/// Determine which evidence layers are applicable for a row kind.
///
/// - `"visual"` (default): L0-L6 (all layers)
/// - `"journey"`: L0, L3, L6 (nonvisual: CLI/backend behavior)
/// - `"terminal_capability"`: L0, L1, L2, L3 (mode negotiation, no pixel diffs)
pub fn applicable_layers_for(row_kind: &str) -> BTreeSet<EvidenceLayer> {
    match row_kind {
        "journey" => [EvidenceLayer::L0, EvidenceLayer::L3, EvidenceLayer::L6].into(),
        "terminal_capability" => [
            EvidenceLayer::L0,
            EvidenceLayer::L1,
            EvidenceLayer::L2,
            EvidenceLayer::L3,
        ]
        .into(),
        _ => [
            EvidenceLayer::L0,
            EvidenceLayer::L1,
            EvidenceLayer::L2,
            EvidenceLayer::L3,
            EvidenceLayer::L4,
            EvidenceLayer::L5,
            EvidenceLayer::L6,
        ]
        .into(),
    }
}

/// Determine which proof dimensions are applicable for a row kind.
///
/// - `"visual"` (default): P0-P9 (all dimensions)
/// - `"journey"`: P0, P3, P7, P9 (inventory, terminal, lifecycle, review)
/// - `"terminal_capability"`: P0, P1, P2, P3 (mode negotiation, no pixel diffs)
pub fn applicable_dimensions_for(row_kind: &str) -> BTreeSet<ProofDimension> {
    match row_kind {
        "journey" => [
            ProofDimension::P0,
            ProofDimension::P3,
            ProofDimension::P7,
            ProofDimension::P9,
        ]
        .into(),
        "terminal_capability" => [
            ProofDimension::P0,
            ProofDimension::P1,
            ProofDimension::P2,
            ProofDimension::P3,
        ]
        .into(),
        _ => ProofDimension::all().into(),
    }
}

/// Check layer completeness: every applicable layer must be present.
pub fn check_layer_completeness(
    applicable: &BTreeSet<EvidenceLayer>,
    present: &BTreeSet<EvidenceLayer>,
) -> LayerCompleteness {
    let missing: BTreeSet<_> = applicable.difference(present).copied().collect();
    let is_complete = missing.is_empty();
    LayerCompleteness {
        applicable_layers: applicable.clone(),
        present_layers: present.clone(),
        missing_layers: missing,
        is_complete,
    }
}

/// Check proof-dimension completeness: every applicable dimension must be present.
pub fn check_dimension_completeness(
    applicable: &BTreeSet<ProofDimension>,
    present: &BTreeSet<ProofDimension>,
) -> DimensionCompleteness {
    let missing: BTreeSet<_> = applicable.difference(present).copied().collect();
    let is_complete = missing.is_empty();
    DimensionCompleteness {
        applicable_dimensions: applicable.clone(),
        present_dimensions: present.clone(),
        missing_dimensions: missing,
        is_complete,
    }
}

/// Reject a `pass` claim unless all required layers and receipt invariants hold.
pub fn validate_pass_status(
    completeness: &LayerCompleteness,
    receipt: &ArtifactReceipt,
) -> Result<(), PassStatusError> {
    if !completeness.is_complete {
        return Err(PassStatusError::MissingEvidence(
            completeness.missing_layers.clone(),
        ));
    }
    if receipt.validate().outcome != ValidationOutcome::Pass {
        return Err(PassStatusError::InvalidReceipt);
    }
    Ok(())
}

/// Reject a `pass` claim unless all required proof dimensions and receipt
/// invariants hold.
pub fn validate_pass_status_with_dimensions(
    completeness: &DimensionCompleteness,
    receipt: &ArtifactReceipt,
) -> Result<(), PassStatusError> {
    if !completeness.is_complete {
        return Err(PassStatusError::MissingDimensions(
            completeness.missing_dimensions.clone(),
        ));
    }
    if receipt.validate().outcome != ValidationOutcome::Pass {
        return Err(PassStatusError::InvalidReceipt);
    }
    Ok(())
}
