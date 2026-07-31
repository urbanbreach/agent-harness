//! Fail-closed schema validation for live-provider and dogfood journey
//! artifact receipts (Todo 12).
//!
//! Every live-provider smoke or dogfood journey must produce an
//! [`ArtifactReceipt`] that carries binary digest, source revision, exact
//! command, provider/auth mode, workspace before/after state, teardown
//! receipt, isolation root, and secret-scan result. The [`ArtifactReceipt::validate`]
//! method is fail-closed: any missing required field, stale isolation root,
//! failed teardown, or secret-bearing value causes a [`ValidationOutcome::Fail`].
//!
//! This module is schema-only. It does not execute providers, network calls,
//! hooks, MCP, or the CLI. It does not claim live provider transport success.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

mod validation;

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

/// Schema version written into artifact receipt JSON.
///
/// v3 adds proof dimensions (P0-P9), reference identity, and epoch bindings.
/// Migration from v2: new fields deserialize with defaults; validation
/// rejects receipts where epoch or proof-dimension fields are empty.
pub const ARTIFACT_RECEIPT_SCHEMA_VERSION: &str = "artifact-receipt-v3";

// ---------------------------------------------------------------------------
// Enums
// ---------------------------------------------------------------------------

/// Provider mode under which the journey was executed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderMode {
    /// Offline / mock provider (no network).
    Offline,
    /// Live provider transport (network required).
    Live,
    /// Cassette replay (deterministic, no network).
    Cassette,
    /// Unknown / unset — the validator rejects this.
    Unknown,
}

/// Authentication mode used for the journey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    /// No authentication (offline / mock).
    None,
    /// API key authentication.
    ApiKey,
    /// OAuth / token-based authentication.
    Oauth,
    /// Unknown / unset — the validator rejects this.
    Unknown,
}

/// Validation outcome for an artifact receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationOutcome {
    /// All required fields present, no secrets, teardown clean.
    Pass,
    /// One or more required fields missing, rejected, or secret-bearing.
    Fail,
}

impl fmt::Display for ValidationOutcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pass => write!(f, "pass"),
            Self::Fail => write!(f, "fail"),
        }
    }
}

// ---------------------------------------------------------------------------
// Sub-structs
// ---------------------------------------------------------------------------

/// Workspace state snapshot (before or after journey execution).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceState {
    /// SHA-256 digest of the workspace tree.
    pub digest: String,
    /// Number of tracked files in the snapshot.
    pub file_count: u32,
}

/// Teardown receipt proving cleanup after journey execution.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeardownReceipt {
    /// Exit code of the teardown process (0 = success).
    pub exit_code: i32,
    /// Paths removed during teardown.
    pub removed_paths: Vec<String>,
    /// Whether the workspace was restored to its pre-journey state.
    pub workspace_restored: bool,
}

/// Secret scan result for the receipt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretScanResult {
    /// True when no secrets were found.
    pub clean: bool,
    /// Patterns that were checked.
    pub patterns_checked: Vec<String>,
    /// Findings (empty when clean).
    pub findings: Vec<SecretFinding>,
}

/// A single secret scan finding.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecretFinding {
    /// Field in the receipt where the secret was found.
    pub field: String,
    /// Pattern that matched.
    pub pattern: String,
    /// Redacted snippet of the matching value.
    pub snippet: String,
}

/// Immutable identity of the executable that created evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunnerIdentity {
    /// Absolute path of the executable invoked by the recorded command.
    pub path: String,
    /// SHA-256 digest of the executable.
    pub sha256: String,
    /// Version emitted by the executable itself.
    pub version: String,
    /// Filesystem permissions recorded at invocation time.
    pub permissions: String,
}

/// Immutable identity shared by every receipt for one candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CandidateIdentity {
    /// Candidate source revision.
    pub source_revision: String,
    /// Candidate binary SHA-256 digest.
    pub binary_digest: String,
}

/// Canonical location and freshness facts for task evidence.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvidenceIdentity {
    /// Attempt namespace, such as `attempt-2`.
    pub attempt_id: String,
    /// Positive task number that owns the evidence directory.
    pub task_id: u32,
    /// Absolute canonical task directory.
    pub root: String,
    /// Absolute path of the artifact written within `root`.
    pub artifact_path: String,
    /// SHA-256 digest of the artifact at creation time.
    pub artifact_sha256: String,
    /// Whether the task root was created empty for this receipt.
    pub fresh_root: bool,
}

/// Immutable identity of the pinned reference binary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReferenceIdentity {
    /// Absolute path of the reference binary.
    pub path: String,
    /// SHA-256 digest of the reference binary.
    pub sha256: String,
}

/// Product and reference epoch bindings for cross-epoch rejection.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpochBindings {
    /// SHA-256 of the canonical product input manifest.
    pub product_epoch: String,
    /// SHA-256 of the canonical reference source manifest.
    pub reference_epoch: String,
}

// ---------------------------------------------------------------------------
// ArtifactReceipt
// ---------------------------------------------------------------------------

/// Fail-closed artifact receipt for live-provider and dogfood journeys.
///
/// Every field is required. The [`ArtifactReceipt::validate`] method returns a
/// [`ValidationResult`] with `.outcome`, `.required_fields_missing`,
/// `.rejected_fields`, and `.secret_scan_clean`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReceipt {
    /// SHA-256 digest of the binary that produced the artifact.
    pub binary_digest: String,
    /// Git revision (commit hash) of the source checkout.
    pub source_revision: String,
    /// Exact command that was executed.
    pub command: String,
    /// Provider mode (offline, live, cassette).
    pub provider_mode: ProviderMode,
    /// Authentication mode (none, api_key, oauth).
    pub auth_mode: AuthMode,
    /// Workspace state before journey execution.
    pub workspace_before: WorkspaceState,
    /// Workspace state after journey execution.
    pub workspace_after: WorkspaceState,
    /// Teardown receipt proving cleanup.
    pub teardown: TeardownReceipt,
    /// Isolation root (absolute path to the isolated workspace).
    pub isolation_root: String,
    /// Secret scan result.
    pub secret_scan: SecretScanResult,
    /// Named owner responsible for this task evidence.
    pub owner: String,
    /// Immutable candidate identity, which must match the receipt identity.
    pub candidate: CandidateIdentity,
    /// Exact executable identity captured when the artifact was created.
    pub runner: RunnerIdentity,
    /// Canonical evidence location and freshness facts.
    pub evidence: EvidenceIdentity,
    /// Immutable identity of the pinned reference binary.
    #[serde(default)]
    pub reference: ReferenceIdentity,
    /// Product and reference epoch bindings.
    #[serde(default)]
    pub epoch: EpochBindings,
    /// Applicable proof dimensions (P0-P9) for this receipt.
    #[serde(default)]
    pub proof_dimensions: BTreeSet<super::status::ProofDimension>,
}

/// Machine-readable validation result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Overall outcome (pass / fail).
    pub outcome: ValidationOutcome,
    /// Names of required fields that are missing or empty.
    pub required_fields_missing: Vec<String>,
    /// Names of fields that were rejected (stale root, secret-bearing, etc.).
    pub rejected_fields: Vec<String>,
    /// Whether the secret scan reported clean.
    pub secret_scan_clean: bool,
    /// Schema version.
    pub schema_version: String,
}

impl ValidationResult {
    /// Serialize the validation result to a JSON string.
    pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

// ---------------------------------------------------------------------------
// JourneyReceipt (wraps ArtifactReceipt with journey metadata)
// ---------------------------------------------------------------------------

/// A journey receipt wraps an [`ArtifactReceipt`] with journey-level metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JourneyReceipt {
    /// Unique journey identifier.
    pub journey_id: String,
    /// Provider mode for this journey.
    pub provider_mode: ProviderMode,
    /// Auth mode for this journey.
    pub auth_mode: AuthMode,
    /// The underlying artifact receipt.
    pub artifact: ArtifactReceipt,
}

impl JourneyReceipt {
    /// Validate the journey receipt by delegating to the inner artifact receipt.
    pub fn validate(&self) -> ValidationResult {
        validation::validate_journey_receipt(self)
    }
}

// ---------------------------------------------------------------------------
// Validation implementation (fail-closed)
// ---------------------------------------------------------------------------

/// Secret patterns that must not appear in any receipt field value.
const SECRET_PATTERNS: &[&str] = &[
    "api_key=",
    "apikey=",
    "bearer ",
    "bearer=",
    "authorization:",
    "authorization=",
    "password=",
    "secret=",
    "token=",
    "sk-",
    "sk_",
    "BEGIN RSA PRIVATE KEY",
    "BEGIN PRIVATE KEY",
    "BEGIN OPENSSH PRIVATE KEY",
];

impl ArtifactReceipt {
    /// Validate the receipt (fail-closed).
    ///
    /// Returns a [`ValidationResult`] with:
    /// - `.outcome` — `Pass` or `Fail`
    /// - `.required_fields_missing` — names of missing/empty required fields
    /// - `.rejected_fields` — names of fields rejected for stale root, secrets, etc.
    /// - `.secret_scan_clean` — whether the secret scan reported clean
    pub fn validate(&self) -> ValidationResult {
        validation::validate_artifact_receipt(self)
    }
}
